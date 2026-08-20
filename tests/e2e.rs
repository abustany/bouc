//! Drives the whole app in a real browser. Needs `chromedriver` on the PATH and
//! a Chrome install, and skips itself when chromedriver is missing. Setting
//! `CHROME_BINARY` picks the browser to drive and makes a missing chromedriver
//! an error instead of a skip, so the nix check cannot silently pass.

use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use fantoccini::actions::{InputSource, MouseActions, PointerAction};
use fantoccini::elements::Element;
use fantoccini::{Client, ClientBuilder, Locator};
use hyper_util::client::legacy::connect::HttpConnector;
use jiff::tz::TimeZone;
use serde_json::{Value, json};

use bouc::sqlite::MEMORY_DB;
use bouc::start;
use bouc::strings::{Locale, Strings};

const WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

const LOGGED_IN_INFO: &str = "#logged-in-info";

const BOOKING_LOG: &str = "[role='region'][aria-label='Booking log']";
const CALENDAR_LINK: &str = "a[href='/']";
const LOG_LINK: &str = "a[href='/log']";
const VISIBLE_POPOVER_CLOSE: &str =
    "[role='dialog'][aria-hidden='false'] button[aria-label='Close']";

/// Server-side `max_capacity`, reaching it turns the day cell red.
const MAX_CAPACITY: u32 = 6;

#[derive(Clone, Copy, Debug)]
enum Modal {
    Booking,
    Login,
}

impl Modal {
    fn xpath(self) -> &'static str {
        match self {
            Self::Booking => {
                r#"//dialog[@aria-labelledby = .//*[@id and (normalize-space(.) = "New booking" or normalize-space(.) = "Edit booking")]/@id]"#
            }
            Self::Login => {
                r#"//dialog[@aria-labelledby = .//*[@id and normalize-space(.) = "What's your name?"]/@id]"#
            }
        }
    }
}

const BOOKING_MODAL: Modal = Modal::Booking;
const LOGIN_MODAL: Modal = Modal::Login;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserProfile {
    Desktop,
    Mobile,
}

impl BrowserProfile {
    fn chrome_options(self) -> Value {
        let mut args = vec![
            "--headless=new",
            "--no-sandbox",
            "--disable-dev-shm-usage",
            "--disable-gpu",
            "--lang=en-US",
        ];

        match self {
            Self::Desktop => {
                args.push("--window-size=1440,900");
            }
            Self::Mobile => {
                args.push("--window-size=390,844");
            }
        }

        let mut options = json!({ "args": args });
        if self == Self::Mobile {
            options["mobileEmulation"] = json!({
                "deviceMetrics": {
                    "width": 390,
                    "height": 844,
                    "pixelRatio": 3.0,
                    "touch": true,
                    "mobile": true,
                },
            });
        }

        options
    }

    async fn select_end_day(self, client: &Client, day: &str) -> Result<()> {
        if self == Self::Desktop {
            hover(client, &day_button(day)).await?;
        }

        click(client, &day_button(day)).await
    }

    async fn wait_for_open_popover(self, client: &Client, selector: &str) -> Result<()> {
        wait_for_visible(client, selector).await?;
        wait_for_displayed_count(
            client,
            &format!("{selector} button[aria-label='Close']"),
            usize::from(self == Self::Mobile),
        )
        .await?;
        wait_for_animations(client, selector).await
    }

    async fn dismiss_popovers(self, client: &Client) -> Result<()> {
        match self {
            Self::Desktop => hover(client, "h1").await?,
            Self::Mobile if visible_popover(client).await? => {
                click(client, VISIBLE_POPOVER_CLOSE).await?;
            }
            Self::Mobile => {}
        }

        wait_for(
            client,
            "the day popovers to hide",
            "return document.querySelector('[role=\"dialog\"][aria-hidden=\"false\"]') === null;",
            vec![],
        )
        .await
    }
}

#[tokio::test]
async fn books_edits_and_deletes_a_booking_on_desktop() -> Result<()> {
    run_scenario(BrowserProfile::Desktop).await
}

#[tokio::test]
async fn books_edits_and_deletes_a_booking_on_mobile() -> Result<()> {
    run_scenario(BrowserProfile::Mobile).await
}

async fn run_scenario(profile: BrowserProfile) -> Result<()> {
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
    let client = new_client(driver.port, browser.as_deref(), profile).await?;

    let result = scenario(&client, addr, profile)
        .await
        .with_context(|| format!("running the {profile:?} scenario"));
    client.close().await.context("closing browser")?;
    result
}

async fn scenario(client: &Client, addr: SocketAddr, profile: BrowserProfile) -> Result<()> {
    let s = Locale::En.strings();
    let (start, middle, end) = (booking_day(10)?, booking_day(11)?, booking_day(12)?);
    let days = [start.as_str(), middle.as_str(), end.as_str()];
    let name_input = "input[name='name']";
    let guest_count_input = "input[name='guest_count']";

    client
        .goto(&format!("http://{addr}/"))
        .await
        .context("loading the index page")?;

    pick_days(client, &start, &end, profile).await?;
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
    assert!(value_missing(client, name_input).await?);

    log_in(client, "Alice").await?;
    wait_for_modal(client, BOOKING_MODAL, true).await?;

    watch_requests(client).await?;

    clear(client, guest_count_input).await?;
    submit(client, BOOKING_MODAL).await?;
    assert!(
        !requested(client).await?,
        "the booking form was submitted without a guest count"
    );
    assert!(value_missing(client, guest_count_input).await?);

    fill(client, guest_count_input, "3").await?;
    submit(client, BOOKING_MODAL).await?;
    wait_for_modal(client, BOOKING_MODAL, false).await?;

    for day in days {
        wait_for_attribute_contains(client, &day_button(day), "aria-label", &s.guests(3), true)
            .await?;
        wait_for_attribute_contains(
            client,
            &day_button(day),
            "aria-label",
            s.day_at_capacity,
            false,
        )
        .await?;
    }

    let entry = single_popover_entry(client, &start, profile).await?;
    assert!(
        entry.contains("Alice") && entry.contains(&s.guests(3)),
        "unexpected booking entry: {entry}"
    );

    booking_with_a_session(client, profile).await?;

    // the creator gets the edit and delete buttons
    open_popover(client, &start, profile).await?;
    wait_for_displayed_count(client, &edit_button(&start), 1).await?;
    wait_for_displayed_count(client, &delete_button(&start), 1).await?;

    click(client, &edit_button(&start)).await?;
    wait_for_modal(client, BOOKING_MODAL, true).await?;
    assert!(
        !modal_open(client, LOGIN_MODAL).await?,
        "the name modal was shown while editing with a valid session"
    );
    assert_eq!(value(client, guest_count_input).await?, "3");

    fill(client, guest_count_input, &MAX_CAPACITY.to_string()).await?;
    submit(client, BOOKING_MODAL).await?;
    wait_for_modal(client, BOOKING_MODAL, false).await?;

    for day in days {
        wait_for_attribute_contains(
            client,
            &day_button(day),
            "aria-label",
            s.day_at_capacity,
            true,
        )
        .await?;
    }

    let entry = single_popover_entry(client, &start, profile).await?;
    assert!(
        entry.contains(&s.guests(MAX_CAPACITY)),
        "the edited guest count is missing from: {entry}"
    );

    // neither a visitor without a session nor another user may touch the booking
    log_out(client, s.profile_login, profile).await?;
    open_popover(client, &start, profile).await?;
    wait_for_displayed_count(client, &edit_button(&start), 0).await?;
    wait_for_displayed_count(client, &delete_button(&start), 0).await?;

    open_login_modal(client, profile).await?;
    log_in(client, "Bob").await?;
    assert!(
        !modal_open(client, BOOKING_MODAL).await?,
        "logging in from the header opened the booking modal"
    );

    open_popover(client, &start, profile).await?;
    wait_for_displayed_count(client, &edit_button(&start), 0).await?;
    wait_for_displayed_count(client, &delete_button(&start), 0).await?;

    log_out(client, s.profile_login, profile).await?;
    open_login_modal(client, profile).await?;
    log_in(client, "Alice").await?;

    open_popover(client, &start, profile).await?;
    wait_for_displayed_count(client, &delete_button(&start), 1).await?;
    click(client, &delete_button(&start)).await?;

    for day in days {
        wait_for_attribute_contains(
            client,
            &day_button(day),
            "aria-label",
            s.day_popover_empty,
            true,
        )
        .await?;
    }
    wait_for_count(client, &format!("{} li", popover(&start)), 0).await?;

    booking_log(client, s, profile).await?;

    Ok(())
}

/// The log page lists the three changes made to the booking above, newest
/// first.
async fn booking_log(client: &Client, s: &Strings, profile: BrowserProfile) -> Result<()> {
    profile.dismiss_popovers(client).await?;
    click(client, LOG_LINK).await?;

    wait_for_count(client, &format!("{BOOKING_LOG} > article"), 3).await?;
    wait_for_attribute(client, LOG_LINK, "aria-current", Some("page")).await?;
    wait_for_attribute(client, CALENDAR_LINK, "aria-current", None).await?;
    wait_for_text(client, LOGGED_IN_INFO, "Alice").await?;

    assert_eq!(
        texts(client, &format!("{BOOKING_LOG} > article > h2")).await?,
        vec![
            s.log_booking_deleted_title("Alice"),
            s.log_booking_changed_title("Alice"),
            s.log_booking_created_title("Alice"),
        ]
    );

    let dates = texts(client, &format!("{BOOKING_LOG} > article > time")).await?;
    assert!(
        dates.iter().all(|d| !d.trim().is_empty()),
        "a log entry has no date: {dates:?}"
    );

    let details = texts(client, &format!("{BOOKING_LOG} > article")).await?;
    let (deleted, changed, created) = (&details[0], &details[1], &details[2]);
    assert!(
        deleted.contains(&s.guests(MAX_CAPACITY)),
        "the deleted booking is missing its guest count: {deleted}"
    );
    assert!(
        changed.contains(&s.guests(3)) && changed.contains(&s.guests(MAX_CAPACITY)),
        "the changed booking is missing a guest count: {changed}"
    );
    assert!(
        created.contains(&s.guests(3)),
        "the created booking is missing its guest count: {created}"
    );

    click(client, CALENDAR_LINK).await?;
    wait_for_count(client, "[role=region][aria-label=Calendars]", 1).await?;
    wait_for_attribute(client, CALENDAR_LINK, "aria-current", Some("page")).await?;

    Ok(())
}

/// A logged in user goes straight to the booking form, the name modal stays
/// out of the way. The booking is dropped instead of saved to leave the rest
/// of the calendar alone.
async fn booking_with_a_session(client: &Client, profile: BrowserProfile) -> Result<()> {
    let (start, end) = (booking_day(20)?, booking_day(21)?);

    pick_days(client, &start, &end, profile).await?;
    wait_for_modal(client, BOOKING_MODAL, true).await?;
    assert!(
        !modal_open(client, LOGIN_MODAL).await?,
        "the name modal was shown to a user with a valid session"
    );

    close_modal(client, BOOKING_MODAL).await?;
    wait_for_modal(client, BOOKING_MODAL, false).await?;
    wait_for_attribute_contains(
        client,
        &day_button(&start),
        "aria-label",
        Locale::En.strings().day_popover_empty,
        true,
    )
    .await?;

    Ok(())
}

async fn log_in(client: &Client, name: &str) -> Result<()> {
    fill(client, "input[name='name']", name).await?;
    submit(client, LOGIN_MODAL).await?;
    wait_for_modal(client, LOGIN_MODAL, false).await?;
    wait_for_text(client, LOGGED_IN_INFO, name).await
}

async fn log_out(client: &Client, login_label: &str, profile: BrowserProfile) -> Result<()> {
    profile.dismiss_popovers(client).await?;
    click(
        client,
        &format!("{LOGGED_IN_INFO} button[title='Disconnect']"),
    )
    .await?;
    wait_for_text(client, LOGGED_IN_INFO, login_label).await
}

async fn open_login_modal(client: &Client, profile: BrowserProfile) -> Result<()> {
    profile.dismiss_popovers(client).await?;
    click_button_named(client, LOGGED_IN_INFO, "Login").await?;
    wait_for_modal(client, LOGIN_MODAL, true).await
}

async fn pick_days(client: &Client, start: &str, end: &str, profile: BrowserProfile) -> Result<()> {
    open_popover(client, start, profile).await?;
    click_button_named(client, &popover(start), "Book…").await?;
    profile.select_end_day(client, end).await
}

async fn open_popover(client: &Client, day: &str, profile: BrowserProfile) -> Result<()> {
    profile.dismiss_popovers(client).await?;
    click(client, &day_button(day)).await?;
    profile.wait_for_open_popover(client, &popover(day)).await
}

/// Saving a booking closes the day popover, so it has to be reopened before
/// its entries can be read.
async fn single_popover_entry(
    client: &Client,
    day: &str,
    profile: BrowserProfile,
) -> Result<String> {
    open_popover(client, day, profile).await?;
    wait_for_count(client, &format!("{} li", popover(day)), 1).await?;

    let entries = texts(client, &format!("{} li", popover(day))).await?;
    entries
        .into_iter()
        .next()
        .context("no booking entry in the popover")
}

fn day_button(day: &str) -> String {
    format!("time[datetime='{day}'] > button")
}

fn popover(day: &str) -> String {
    format!("[role='dialog']:has(time[datetime='{day}'])")
}

fn edit_button(day: &str) -> String {
    format!("{} li button[title='Edit']", popover(day))
}

fn delete_button(day: &str) -> String {
    format!("{} li button[title='Delete']", popover(day))
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
        .strftime("%F")
        .to_string())
}

async fn serve() -> Result<SocketAddr> {
    let port = free_port()?;
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    let signed_cookie_key = vec![0u8; 64];

    tokio::spawn(async move {
        start(MEMORY_DB, addr, &signed_cookie_key, Some(TimeZone::UTC), 6)
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

async fn new_client(port: u16, browser: Option<&str>, profile: BrowserProfile) -> Result<Client> {
    // the nix build sandbox has neither a user namespace nor a large /dev/shm.
    // The desktop window keeps the booked month in the first calendar row so
    // scrolling does not move the fixed popovers while chromedriver aims at them.
    let mut chrome_options = profile.chrome_options();

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

async fn click_button_named(client: &Client, scope: &str, name: &str) -> Result<()> {
    let scope = client
        .find(Locator::Css(scope))
        .await
        .with_context(|| format!("finding {scope}"))?;
    scope
        .find(Locator::XPath(&format!(
            ".//button[normalize-space(.)='{name}']"
        )))
        .await
        .with_context(|| format!("finding the {name} button"))?
        .click()
        .await
        .with_context(|| format!("clicking the {name} button"))?;
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

async fn find_modal(client: &Client, modal: Modal) -> Result<Element> {
    client
        .find(Locator::XPath(modal.xpath()))
        .await
        .with_context(|| format!("finding the {modal:?} modal by its accessible name"))
}

async fn submit(client: &Client, modal: Modal) -> Result<()> {
    find_modal(client, modal)
        .await?
        .find(Locator::XPath(".//button[normalize-space(.)='Save']"))
        .await
        .with_context(|| format!("finding the Save button in the {modal:?} modal"))?
        .click()
        .await
        .with_context(|| format!("submitting the {modal:?} modal"))?;
    Ok(())
}

async fn close_modal(client: &Client, modal: Modal) -> Result<()> {
    find_modal(client, modal)
        .await?
        .find(Locator::Css("button[aria-label='Close']"))
        .await
        .with_context(|| format!("finding the close button in the {modal:?} modal"))?
        .click()
        .await
        .with_context(|| format!("closing the {modal:?} modal"))?;
    Ok(())
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

async fn visible_popover(client: &Client) -> Result<bool> {
    eval(
        client,
        "return document.querySelector('[role=\"dialog\"][aria-hidden=\"false\"]') !== null;",
        vec![],
    )
    .await?
    .as_bool()
    .context("the visible popover state is not a boolean")
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

async fn modal_open(client: &Client, modal: Modal) -> Result<bool> {
    eval(
        client,
        "return document.evaluate(arguments[0], document, null, \
             XPathResult.FIRST_ORDERED_NODE_TYPE).singleNodeValue.open;",
        vec![json!(modal.xpath())],
    )
    .await?
    .as_bool()
    .with_context(|| format!("the open state of the {modal:?} modal is not a boolean"))
}

async fn wait_for_modal(client: &Client, modal: Modal, open: bool) -> Result<()> {
    wait_for(
        client,
        &format!(
            "the {modal:?} modal to be {}",
            state(open, "open", "closed")
        ),
        "const e = document.evaluate(arguments[0], document, null, \
             XPathResult.FIRST_ORDERED_NODE_TYPE).singleNodeValue; \
         return !!e && e.open === arguments[1];",
        vec![json!(modal.xpath()), json!(open)],
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

async fn wait_for_animations(client: &Client, selector: &str) -> Result<()> {
    wait_for(
        client,
        &format!("animations on {selector} to settle"),
        "const e = document.querySelector(arguments[0]); \
         return !!e && e.getAnimations({ subtree: true }).every(animation => \
             animation.playState === 'finished' || animation.playState === 'idle' \
         );",
        vec![json!(selector)],
    )
    .await
}

async fn wait_for_attribute(
    client: &Client,
    selector: &str,
    attribute: &str,
    value: Option<&str>,
) -> Result<()> {
    wait_for(
        client,
        &format!("{selector} to have {attribute}={value:?}"),
        "const e = document.querySelector(arguments[0]); \
         return !!e && e.getAttribute(arguments[1]) === arguments[2];",
        vec![json!(selector), json!(attribute), json!(value)],
    )
    .await
}

async fn wait_for_attribute_contains(
    client: &Client,
    selector: &str,
    attribute: &str,
    value: &str,
    present: bool,
) -> Result<()> {
    wait_for(
        client,
        &format!(
            "{selector}'s {attribute} to {} {value}",
            state(present, "contain", "not contain")
        ),
        "const e = document.querySelector(arguments[0]); \
         return !!e && e.getAttribute(arguments[1]).includes(arguments[2]) === arguments[3];",
        vec![
            json!(selector),
            json!(attribute),
            json!(value),
            json!(present),
        ],
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
