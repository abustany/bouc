//! Drives the whole app in a real browser. Needs `chromedriver` on the PATH and
//! a Chrome install, and skips itself when chromedriver is missing. Setting
//! `CHROME_BINARY` picks the browser to drive and makes a missing chromedriver
//! an error instead of a skip, so the nix check cannot silently pass.

use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use fantoccini::actions::{InputSource, MouseActions, PointerAction};
use fantoccini::{Client, ClientBuilder, Locator};
use hyper_util::client::legacy::connect::HttpConnector;
use serde_json::{Value, json};

use bouc::sqlite::MEMORY_DB;
use bouc::start;
use bouc::strings::Locale;

const WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

const BOOKED_CLASS: &str = "border-b-amber-300";
const FULL_CLASS: &str = "border-b-red-600";

/// The page holds one dialog per modal, told apart by the form they submit.
const BOOKING_MODAL: &str = "dialog:has(form[action='/bookings'])";
const LOGIN_MODAL: &str = "dialog:has(form[action='/login'])";
const LOGGED_IN_INFO: &str = "#logged-in-info";

/// Server-side `max_capacity`, reaching it turns the day cell red.
const MAX_CAPACITY: u32 = 6;

#[tokio::test]
async fn books_edits_and_deletes_a_booking() -> Result<()> {
    let browser = std::env::var("CHROME_BINARY").ok();

    if !chromedriver_available() {
        if let Some(browser) = browser {
            bail!("CHROME_BINARY is set to {browser} but chromedriver is not in the PATH");
        }
        eprintln!("skipping e2e test: chromedriver not found in PATH");
        return Ok(());
    }

    let addr = serve().await?;
    let driver = Chromedriver::start().await?;
    let client = new_client(driver.port, browser.as_deref()).await?;

    let result = scenario(&client, addr).await;
    client.close().await.context("closing browser")?;
    result
}

async fn scenario(client: &Client, addr: SocketAddr) -> Result<()> {
    let s = Locale::En.strings();
    let (start, middle, end) = (booking_day(10)?, booking_day(11)?, booking_day(12)?);
    let days = [start.as_str(), middle.as_str(), end.as_str()];
    let name_input = format!("{LOGIN_MODAL} input[name='name']");
    let guest_count_input = format!("{BOOKING_MODAL} input[name='guest_count']");

    client
        .goto(&format!("http://{addr}/"))
        .await
        .context("loading the index page")?;

    pick_days(client, &start, &end).await?;
    wait_for_modal(client, LOGIN_MODAL, true).await?;
    assert!(
        !modal_open(client, BOOKING_MODAL).await?,
        "the booking modal was shown to a visitor without a session"
    );

    watch_requests(client).await?;

    submit(client, LOGIN_MODAL).await?;
    assert!(
        !requested(client).await?,
        "the login form was submitted without a name"
    );
    assert!(value_missing(client, &name_input).await?);

    log_in(client, "Alice").await?;
    wait_for_modal(client, BOOKING_MODAL, true).await?;

    watch_requests(client).await?;

    clear(client, &guest_count_input).await?;
    submit(client, BOOKING_MODAL).await?;
    assert!(
        !requested(client).await?,
        "the booking form was submitted without a guest count"
    );
    assert!(value_missing(client, &guest_count_input).await?);

    fill(client, &guest_count_input, "3").await?;
    submit(client, BOOKING_MODAL).await?;
    wait_for_modal(client, BOOKING_MODAL, false).await?;

    for day in days {
        wait_for_class(client, &cell(day), BOOKED_CLASS, true).await?;
        wait_for_class(client, &cell(day), FULL_CLASS, false).await?;
    }

    let entry = single_popover_entry(client, &start).await?;
    assert!(
        entry.contains("Alice") && entry.contains(&s.guests(3)),
        "unexpected booking entry: {entry}"
    );

    booking_with_a_session(client).await?;

    // the creator gets the edit and delete buttons
    open_popover(client, &start).await?;
    wait_for_displayed_count(client, &edit_button(&start), 1).await?;
    wait_for_displayed_count(client, &delete_button(&start), 1).await?;

    click(client, &edit_button(&start)).await?;
    wait_for_modal(client, BOOKING_MODAL, true).await?;
    assert!(
        !modal_open(client, LOGIN_MODAL).await?,
        "the name modal was shown while editing with a valid session"
    );
    assert_eq!(value(client, &guest_count_input).await?, "3");

    fill(client, &guest_count_input, &MAX_CAPACITY.to_string()).await?;
    submit(client, BOOKING_MODAL).await?;
    wait_for_modal(client, BOOKING_MODAL, false).await?;

    for day in days {
        wait_for_class(client, &cell(day), FULL_CLASS, true).await?;
    }

    let entry = single_popover_entry(client, &start).await?;
    assert!(
        entry.contains(&s.guests(MAX_CAPACITY)),
        "the edited guest count is missing from: {entry}"
    );

    // neither a visitor without a session nor another user may touch the booking
    log_out(client, s.profile_login).await?;
    open_popover(client, &start).await?;
    wait_for_displayed_count(client, &edit_button(&start), 0).await?;
    wait_for_displayed_count(client, &delete_button(&start), 0).await?;

    open_login_modal(client).await?;
    log_in(client, "Bob").await?;
    assert!(
        !modal_open(client, BOOKING_MODAL).await?,
        "logging in from the header opened the booking modal"
    );

    open_popover(client, &start).await?;
    wait_for_displayed_count(client, &edit_button(&start), 0).await?;
    wait_for_displayed_count(client, &delete_button(&start), 0).await?;

    log_out(client, s.profile_login).await?;
    open_login_modal(client).await?;
    log_in(client, "Alice").await?;

    open_popover(client, &start).await?;
    wait_for_displayed_count(client, &delete_button(&start), 1).await?;
    click(client, &delete_button(&start)).await?;

    for day in days {
        wait_for_class(client, &cell(day), BOOKED_CLASS, false).await?;
    }
    wait_for_count(client, &format!("{} li", popover(&start)), 0).await?;

    Ok(())
}

/// A logged in user goes straight to the booking form, the name modal stays
/// out of the way. The booking is dropped instead of saved to leave the rest
/// of the calendar alone.
async fn booking_with_a_session(client: &Client) -> Result<()> {
    let (start, end) = (booking_day(20)?, booking_day(21)?);

    pick_days(client, &start, &end).await?;
    wait_for_modal(client, BOOKING_MODAL, true).await?;
    assert!(
        !modal_open(client, LOGIN_MODAL).await?,
        "the name modal was shown to a user with a valid session"
    );

    close_modal(client, BOOKING_MODAL).await?;
    wait_for_modal(client, BOOKING_MODAL, false).await?;
    wait_for_class(client, &cell(&start), BOOKED_CLASS, false).await?;

    Ok(())
}

async fn log_in(client: &Client, name: &str) -> Result<()> {
    fill(client, &format!("{LOGIN_MODAL} input[name='name']"), name).await?;
    submit(client, LOGIN_MODAL).await?;
    wait_for_modal(client, LOGIN_MODAL, false).await?;
    wait_for_text(client, LOGGED_IN_INFO, name).await
}

async fn log_out(client: &Client, login_label: &str) -> Result<()> {
    dismiss_popovers(client).await?;
    click(
        client,
        &format!("{LOGGED_IN_INFO} button[hx-post='/logout']"),
    )
    .await?;
    wait_for_text(client, LOGGED_IN_INFO, login_label).await
}

async fn open_login_modal(client: &Client) -> Result<()> {
    dismiss_popovers(client).await?;
    click(client, &format!("{LOGGED_IN_INFO} button")).await?;
    wait_for_modal(client, LOGIN_MODAL, true).await
}

async fn pick_days(client: &Client, start: &str, end: &str) -> Result<()> {
    open_popover(client, start).await?;
    click(client, &format!("{} button.btn-primary", popover(start))).await?;
    hover(client, &cell(end)).await?;
    click(client, &day_button(end)).await
}

async fn open_popover(client: &Client, day: &str) -> Result<()> {
    dismiss_popovers(client).await?;
    click(client, &day_button(day)).await?;
    wait_for_visible(client, &popover(day)).await
}

/// A popover left over from a previous day covers the cells below it and would
/// swallow the next click.
async fn dismiss_popovers(client: &Client) -> Result<()> {
    hover(client, "h1").await?;
    wait_for(
        client,
        "the day popovers to hide",
        "return Array.from(document.querySelectorAll('[x-ref^=\"day-popover-\"]')) \
             .every(e => getComputedStyle(e).visibility === 'hidden');",
        vec![],
    )
    .await
}

/// Saving a booking closes the day popover, so it has to be reopened before
/// its entries can be read.
async fn single_popover_entry(client: &Client, day: &str) -> Result<String> {
    open_popover(client, day).await?;
    wait_for_count(client, &format!("{} li", popover(day)), 1).await?;

    let entries = texts(client, &format!("{} li", popover(day))).await?;
    entries
        .into_iter()
        .next()
        .context("no booking entry in the popover")
}

fn cell(day: &str) -> String {
    format!("[x-ref='cell-{day}']")
}

fn day_button(day: &str) -> String {
    format!("{} > button", cell(day))
}

fn popover(day: &str) -> String {
    format!("[x-ref='day-popover-{day}']")
}

fn edit_button(day: &str) -> String {
    format!("{} li button:not([hx-delete])", popover(day))
}

fn delete_button(day: &str) -> String {
    format!("{} li button[hx-delete]", popover(day))
}

fn booking_day(day: i8) -> Result<String> {
    let next_month = jiff::Zoned::now()
        .date()
        .first_of_month()
        .checked_add(jiff::Span::new().months(1))
        .context("computing next month")?;

    Ok(next_month
        .with()
        .day(day)
        .build()
        .context("building booking date")?
        .strftime("%Y%m%d")
        .to_string())
}

async fn serve() -> Result<SocketAddr> {
    let port = free_port()?;
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    let signed_cookie_key = vec![0u8; 64];

    tokio::spawn(async move {
        start(MEMORY_DB, addr, &signed_cookie_key)
            .await
            .expect("serving");
    });

    Ok(addr)
}

fn chromedriver_available() -> bool {
    Command::new("chromedriver")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

struct Chromedriver {
    process: Child,
    port: u16,
}

impl Chromedriver {
    async fn start() -> Result<Self> {
        let port = free_port()?;
        let process = Command::new("chromedriver")
            .arg(format!("--port={port}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("spawning chromedriver")?;
        let driver = Self { process, port };

        let deadline = Instant::now() + WAIT_TIMEOUT;
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            if Instant::now() >= deadline {
                bail!("chromedriver did not start listening on port {port}");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }

        Ok(driver)
    }
}

impl Drop for Chromedriver {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").context("binding to a free port")?;
    Ok(listener.local_addr()?.port())
}

async fn new_client(port: u16, browser: Option<&str>) -> Result<Client> {
    // the nix build sandbox has neither a user namespace nor a large /dev/shm.
    // The window size keeps the booked month in the first calendar row:
    // scrolling would move the fixed popovers while chromedriver aims at them.
    let mut chrome_options = json!({
        "args": [
            "--headless=new",
            "--no-sandbox",
            "--disable-dev-shm-usage",
            "--disable-gpu",
            "--window-size=1440,900",
            "--lang=en-US",
        ],
    });

    if let Some(binary) = browser {
        chrome_options["binary"] = json!(binary);
    }

    let mut capabilities = serde_json::Map::new();
    capabilities.insert("goog:chromeOptions".to_owned(), chrome_options);
    // htmx asks for a confirmation before deleting a booking
    capabilities.insert("unhandledPromptBehavior".to_owned(), json!("accept"));

    let mut builder = ClientBuilder::new(HttpConnector::new());
    builder.capabilities(capabilities);
    builder
        .connect(&format!("http://127.0.0.1:{port}"))
        .await
        .context("connecting to chromedriver")
}

async fn click(client: &Client, selector: &str) -> Result<()> {
    client
        .find(Locator::Css(selector))
        .await
        .with_context(|| format!("finding {selector}"))?
        .click()
        .await
        .with_context(|| format!("clicking {selector}"))?;
    Ok(())
}

async fn hover(client: &Client, selector: &str) -> Result<()> {
    let element = client
        .find(Locator::Css(selector))
        .await
        .with_context(|| format!("finding {selector}"))?;
    client
        .perform_actions(
            MouseActions::new("mouse".to_owned()).then(PointerAction::MoveToElement {
                element,
                duration: None,
                x: 0.0,
                y: 0.0,
            }),
        )
        .await
        .with_context(|| format!("hovering {selector}"))?;
    Ok(())
}

async fn clear(client: &Client, selector: &str) -> Result<()> {
    client
        .find(Locator::Css(selector))
        .await
        .with_context(|| format!("finding {selector}"))?
        .clear()
        .await
        .with_context(|| format!("clearing {selector}"))?;
    Ok(())
}

async fn fill(client: &Client, selector: &str, value: &str) -> Result<()> {
    let element = client
        .find(Locator::Css(selector))
        .await
        .with_context(|| format!("finding {selector}"))?;
    element
        .clear()
        .await
        .with_context(|| format!("clearing {selector}"))?;
    element
        .send_keys(value)
        .await
        .with_context(|| format!("typing into {selector}"))?;
    Ok(())
}

async fn submit(client: &Client, modal: &str) -> Result<()> {
    click(client, &format!("{modal} form button.btn-primary")).await
}

async fn close_modal(client: &Client, modal: &str) -> Result<()> {
    click(client, &format!("{modal} button[aria-label]")).await
}

async fn value(client: &Client, selector: &str) -> Result<String> {
    client
        .find(Locator::Css(selector))
        .await
        .with_context(|| format!("finding {selector}"))?
        .prop("value")
        .await
        .with_context(|| format!("reading the value of {selector}"))?
        .with_context(|| format!("{selector} has no value"))
}

async fn value_missing(client: &Client, selector: &str) -> Result<bool> {
    let missing = eval(
        client,
        "return document.querySelector(arguments[0]).validity.valueMissing;",
        vec![json!(selector)],
    )
    .await?;
    missing
        .as_bool()
        .with_context(|| format!("validity of {selector} is not a boolean"))
}

/// htmx fires `htmx:beforeRequest` while handling the click, so the flag is
/// already set by the time a click command returns.
async fn watch_requests(client: &Client) -> Result<()> {
    eval(
        client,
        "window.sawRequest = false; \
         if (!window.watchingRequests) { \
             window.watchingRequests = true; \
             document.body.addEventListener('htmx:beforeRequest', () => { window.sawRequest = true; }); \
         }",
        vec![],
    )
    .await?;
    Ok(())
}

async fn requested(client: &Client) -> Result<bool> {
    eval(client, "return window.sawRequest;", vec![])
        .await?
        .as_bool()
        .context("the request flag is not a boolean")
}

async fn texts(client: &Client, selector: &str) -> Result<Vec<String>> {
    let texts = eval(
        client,
        "return Array.from(document.querySelectorAll(arguments[0]), e => e.textContent);",
        vec![json!(selector)],
    )
    .await?;
    serde_json::from_value(texts).with_context(|| format!("reading the text of {selector}"))
}

async fn modal_open(client: &Client, modal: &str) -> Result<bool> {
    eval(
        client,
        "return document.querySelector(arguments[0]).open;",
        vec![json!(modal)],
    )
    .await?
    .as_bool()
    .with_context(|| format!("the open state of {modal} is not a boolean"))
}

async fn wait_for_modal(client: &Client, modal: &str, open: bool) -> Result<()> {
    wait_for(
        client,
        &format!("{modal} to be {}", state(open, "open", "closed")),
        "const e = document.querySelector(arguments[0]); \
         return !!e && e.open === arguments[1];",
        vec![json!(modal), json!(open)],
    )
    .await
}

async fn wait_for_visible(client: &Client, selector: &str) -> Result<()> {
    wait_for(
        client,
        &format!("{selector} to become visible"),
        "const e = document.querySelector(arguments[0]); \
         return !!e && getComputedStyle(e).visibility === 'visible';",
        vec![json!(selector)],
    )
    .await
}

async fn wait_for_class(client: &Client, selector: &str, class: &str, present: bool) -> Result<()> {
    wait_for(
        client,
        &format!(
            "{selector} to {} the {class} class",
            state(present, "gain", "lose")
        ),
        "const e = document.querySelector(arguments[0]); \
         return !!e && e.classList.contains(arguments[1]) === arguments[2];",
        vec![json!(selector), json!(class), json!(present)],
    )
    .await
}

async fn wait_for_count(client: &Client, selector: &str, count: usize) -> Result<()> {
    wait_for(
        client,
        &format!("{count} element(s) matching {selector}"),
        "return document.querySelectorAll(arguments[0]).length === arguments[1];",
        vec![json!(selector), json!(count)],
    )
    .await
}

async fn wait_for_displayed_count(client: &Client, selector: &str, count: usize) -> Result<()> {
    wait_for(
        client,
        &format!("{count} displayed element(s) matching {selector}"),
        "return Array.from(document.querySelectorAll(arguments[0])) \
             .filter(e => getComputedStyle(e).display !== 'none').length === arguments[1];",
        vec![json!(selector), json!(count)],
    )
    .await
}

async fn wait_for_text(client: &Client, selector: &str, text: &str) -> Result<()> {
    wait_for(
        client,
        &format!("{selector} to contain {text}"),
        "const e = document.querySelector(arguments[0]); \
         return !!e && e.textContent.includes(arguments[1]);",
        vec![json!(selector), json!(text)],
    )
    .await
}

fn state(value: bool, yes: &'static str, no: &'static str) -> &'static str {
    if value { yes } else { no }
}

async fn wait_for(
    client: &Client,
    description: &str,
    script: &str,
    args: Vec<Value>,
) -> Result<()> {
    let deadline = Instant::now() + WAIT_TIMEOUT;

    loop {
        if eval(client, script, args.clone()).await? == json!(true) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for {description}");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn eval(client: &Client, script: &str, args: Vec<Value>) -> Result<Value> {
    client
        .execute(script, args)
        .await
        .with_context(|| format!("evaluating {script}"))
}
