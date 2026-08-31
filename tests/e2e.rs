//! Drives the whole app in a real browser. Needs `chromedriver` and `mailpit`
//! on the PATH and a Chrome install, and skips itself when chromedriver is
//! missing. Setting `CHROME_BINARY` picks the browser to drive and makes a
//! missing chromedriver an error instead of a skip, so the nix check cannot
//! silently pass.

use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use fantoccini::elements::Element;
use fantoccini::{Client, ClientBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use icu::calendar::{Date as IcuDate, Iso};
use icu::datetime::DateTimeFormatter;
use icu::datetime::fieldsets::MD;
use icu::locale::locale;
use jiff::tz::TimeZone;
use jiff_icu::ConvertInto;
use serde_json::{Value, json};

mod mailpit;
mod testing_library;

use mailpit::Mailpit;
use testing_library::{
    NameMatch, POLL_INTERVAL, User, WAIT_TIMEOUT, eval, screen, texts_in, value, value_missing,
    wait_for_animations, wait_for_attribute, wait_for_count, wait_for_count_in, wait_for_text,
    wait_for_visible_text, within,
};

use bouc::sqlite::MEMORY_DB;
use bouc::strings::{Locale, Strings};
use bouc::{StartOptions, email, start};

const LOGGED_IN_INFO: &str = "#logged-in-info";

/// Server-side `max_capacity`, reaching it turns the day cell red.
const MAX_CAPACITY: u32 = 6;

#[derive(Clone, Copy, Debug)]
enum Modal {
    Booking,
    Login,
    Email,
}

impl Modal {
    fn accessible_names(self) -> &'static [&'static str] {
        match self {
            Self::Booking => &["New booking", "Edit booking"],
            Self::Login => &["What's your name?"],
            Self::Email => &["What's your email?"],
        }
    }
}

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
        let button = find_day_button(client, day).await?;
        let user = User::new(client);

        if self == Self::Desktop {
            user.hover(&button).await?;
        }

        user.click(&button).await
    }

    async fn wait_for_open_popover(self, client: &Client, day: &str) -> Result<Element> {
        let popover = screen(client)
            .find_by_role("dialog", Some(NameMatch::Exact(&accessible_day_name(day)?)))
            .await?;
        within(client, &popover)
            .wait_for_role_count(
                "button",
                Some(NameMatch::Exact("Close")),
                usize::from(self == Self::Mobile),
            )
            .await?;
        wait_for_animations(client, &popover).await?;
        Ok(popover)
    }

    async fn dismiss_popovers(self, client: &Client) -> Result<()> {
        let user = User::new(client);

        match self {
            Self::Desktop => {
                let heading = screen(client)
                    .find_by_role("heading", Some(NameMatch::Contains("Bouc")))
                    .await?;
                user.hover(&heading).await?;
            }
            Self::Mobile => {
                if let Some(popover) = screen(client).query_by_role("dialog", None).await? {
                    let close = within(client, &popover)
                        .find_by_role("button", Some(NameMatch::Exact("Close")))
                        .await?;
                    user.click(&close).await?;
                }
            }
        }

        screen(client).wait_for_role_count("dialog", None, 0).await
    }
}

#[tokio::test]
async fn books_edits_and_deletes_a_booking_on_desktop() -> Result<()> {
    run_scenario(BrowserProfile::Desktop, async |session| {
        scenario(&session.client, session.addr, session.profile).await
    })
    .await
}

#[tokio::test]
async fn books_edits_and_deletes_a_booking_on_mobile() -> Result<()> {
    run_scenario(BrowserProfile::Mobile, async |session| {
        scenario(&session.client, session.addr, session.profile).await
    })
    .await
}

#[tokio::test]
async fn subscribes_to_notifications() -> Result<()> {
    run_scenario(BrowserProfile::Desktop, notifications).await
}

/// A server, a browser and the mail server the app delivers to.
struct Session {
    _driver: Chromedriver,
    client: Client,
    addr: SocketAddr,
    mailpit: Mailpit,
    profile: BrowserProfile,
}

impl Session {
    async fn goto(&self, url: &str) -> Result<()> {
        self.client
            .goto(url)
            .await
            .with_context(|| format!("loading {url}"))
    }

    fn index_url(&self) -> String {
        format!("http://{}/", self.addr)
    }
}

async fn run_scenario(
    profile: BrowserProfile,
    scenario: impl AsyncFnOnce(&Session) -> Result<()>,
) -> Result<()> {
    let browser = std::env::var("CHROME_BINARY").ok();

    if !chromedriver_available() {
        if let Some(browser) = browser {
            bail!("CHROME_BINARY is set to {browser} but chromedriver is not in the PATH");
        }
        eprintln!("skipping e2e test: chromedriver not found in PATH");
        return Ok(());
    }

    let mailpit = Mailpit::start().await?;
    let addr = serve(&mailpit.smtp_url()).await?;
    let driver = Chromedriver::start().await?;
    let client = new_client(driver.port, browser.as_deref(), profile).await?;
    let session = Session {
        _driver: driver,
        client,
        addr,
        mailpit,
        profile,
    };

    let result = scenario(&session)
        .await
        .with_context(|| format!("running the {profile:?} scenario"));
    session.client.close().await.context("closing browser")?;
    result
}

async fn scenario(client: &Client, addr: SocketAddr, profile: BrowserProfile) -> Result<()> {
    let s = Locale::En.strings();
    let (start, middle, end) = (booking_day(10)?, booking_day(11)?, booking_day(12)?);
    let days = [start.as_str(), middle.as_str(), end.as_str()];
    let user = User::new(client);

    client
        .goto(&format!("http://{addr}/"))
        .await
        .context("loading the index page")?;

    pick_days(client, &start, &end, profile).await?;
    let login_modal = find_modal(client, Modal::Login).await?;
    assert!(
        !modal_open(client, Modal::Booking).await?,
        "the booking modal was shown to a visitor without a session"
    );

    watch_requests(client).await?;

    submit(client, Modal::Login).await?;
    assert!(
        !requested(client).await?,
        "the login form was submitted without a name"
    );
    let name_input = within(client, &login_modal)
        .find_by_role("textbox", Some(NameMatch::Contains(s.name_modal_name)))
        .await?;
    assert!(value_missing(&name_input).await?);

    log_in_as(client, "alice", "Alice").await?;
    let booking_modal = find_modal(client, Modal::Booking).await?;
    let guest_count_input = within(client, &booking_modal)
        .find_by_role(
            "spinbutton",
            Some(NameMatch::Contains(s.booking_modal_guest_count)),
        )
        .await?;

    watch_requests(client).await?;

    user.clear(&guest_count_input).await?;
    submit(client, Modal::Booking).await?;
    assert!(
        !requested(client).await?,
        "the booking form was submitted without a guest count"
    );
    assert!(value_missing(&guest_count_input).await?);

    user.fill(&guest_count_input, "3").await?;
    submit(client, Modal::Booking).await?;
    wait_for_modal_to_close(client, Modal::Booking).await?;

    for day in days {
        wait_for_day_name_contains(client, day, &s.guests(3), true).await?;
        wait_for_day_name_contains(client, day, s.day_at_capacity, false).await?;
    }

    let entry = single_popover_entry(client, &start, profile).await?;
    assert!(
        entry.contains("Alice") && entry.contains(&s.guests(3)),
        "unexpected booking entry: {entry}"
    );

    booking_with_a_session(client, profile).await?;

    // the creator gets the edit and delete buttons
    let popover = open_popover(client, &start, profile).await?;
    let edit = within(client, &popover)
        .find_by_role(
            "button",
            Some(NameMatch::Exact(s.day_popover_edit_button_title)),
        )
        .await?;
    within(client, &popover)
        .find_by_role(
            "button",
            Some(NameMatch::Exact(s.day_popover_delete_button_title)),
        )
        .await?;
    user.click(&edit).await?;
    let booking_modal = find_modal(client, Modal::Booking).await?;
    assert!(
        !modal_open(client, Modal::Login).await?,
        "the name modal was shown while editing with a valid session"
    );
    let guest_count_input = within(client, &booking_modal)
        .find_by_role(
            "spinbutton",
            Some(NameMatch::Contains(s.booking_modal_guest_count)),
        )
        .await?;
    assert_eq!(value(&guest_count_input).await?, "3");

    user.fill(&guest_count_input, &MAX_CAPACITY.to_string())
        .await?;
    submit(client, Modal::Booking).await?;
    wait_for_modal_to_close(client, Modal::Booking).await?;

    for day in days {
        wait_for_day_name_contains(client, day, s.day_at_capacity, true).await?;
    }

    let entry = single_popover_entry(client, &start, profile).await?;
    assert!(
        entry.contains(&s.guests(MAX_CAPACITY)),
        "the edited guest count is missing from: {entry}"
    );

    // neither a visitor without a session nor another user may touch the booking
    log_out(client, s.profile_login, profile).await?;
    let popover = open_popover(client, &start, profile).await?;
    wait_for_booking_action_count(client, &popover, s, 0).await?;

    open_login_modal(client, profile).await?;
    log_in(client, "Bob").await?;
    assert!(
        !modal_open(client, Modal::Booking).await?,
        "logging in from the header opened the booking modal"
    );

    let popover = open_popover(client, &start, profile).await?;
    wait_for_booking_action_count(client, &popover, s, 0).await?;

    log_out(client, s.profile_login, profile).await?;
    open_login_modal(client, profile).await?;
    log_in(client, "Alice").await?;

    let popover = open_popover(client, &start, profile).await?;
    let delete = within(client, &popover)
        .find_by_role(
            "button",
            Some(NameMatch::Exact(s.day_popover_delete_button_title)),
        )
        .await?;
    user.click(&delete).await?;

    for day in days {
        wait_for_day_name_contains(client, day, s.day_popover_empty, true).await?;
    }
    wait_for_count(
        client,
        &format!("[role='dialog']:has(time[datetime='{start}']) li"),
        0,
    )
    .await?;

    booking_log(client, s, profile).await?;

    Ok(())
}

/// The log page lists the three changes made to the booking above, newest
/// first.
async fn booking_log(client: &Client, s: &Strings, profile: BrowserProfile) -> Result<()> {
    profile.dismiss_popovers(client).await?;
    let user = User::new(client);
    let log_link = screen(client)
        .find_by_role("link", Some(NameMatch::Contains(s.navbar_link_log)))
        .await?;
    user.click(&log_link).await?;

    let booking_log = screen(client)
        .find_by_role("region", Some(NameMatch::Exact("Booking log")))
        .await?;
    wait_for_count_in(client, &booking_log, ":scope > article", 3).await?;
    let log_link = screen(client)
        .find_by_role("link", Some(NameMatch::Contains(s.navbar_link_log)))
        .await?;
    let calendar_link = screen(client)
        .find_by_role("link", Some(NameMatch::Contains(s.navbar_link_calendar)))
        .await?;
    wait_for_attribute(client, &log_link, "aria-current", Some("page")).await?;
    wait_for_attribute(client, &calendar_link, "aria-current", None).await?;
    wait_for_text(client, LOGGED_IN_INFO, "Alice").await?;

    assert_eq!(
        texts_in(client, &booking_log, ":scope > article > h2").await?,
        vec![
            s.log_booking_deleted_title("Alice"),
            s.log_booking_changed_title("Alice"),
            s.log_booking_created_title("Alice"),
        ]
    );

    let dates = texts_in(client, &booking_log, ":scope > article > time").await?;
    assert!(
        dates.iter().all(|d| !d.trim().is_empty()),
        "a log entry has no date: {dates:?}"
    );

    let details = texts_in(client, &booking_log, ":scope > article").await?;
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

    user.click(&calendar_link).await?;
    screen(client)
        .find_by_role("region", Some(NameMatch::Exact("Calendars")))
        .await?;
    let calendar_link = screen(client)
        .find_by_role("link", Some(NameMatch::Contains(s.navbar_link_calendar)))
        .await?;
    wait_for_attribute(client, &calendar_link, "aria-current", Some("page")).await?;

    Ok(())
}

/// A logged in user goes straight to the booking form, the name modal stays
/// out of the way. The booking is dropped instead of saved to leave the rest
/// of the calendar alone.
async fn booking_with_a_session(client: &Client, profile: BrowserProfile) -> Result<()> {
    let (start, end) = (booking_day(20)?, booking_day(21)?);

    pick_days(client, &start, &end, profile).await?;
    let booking_modal = find_modal(client, Modal::Booking).await?;
    assert!(
        !modal_open(client, Modal::Login).await?,
        "the name modal was shown to a user with a valid session"
    );

    close_modal(client, &booking_modal).await?;
    wait_for_modal_to_close(client, Modal::Booking).await?;
    wait_for_day_name_contains(client, &start, Locale::En.strings().day_popover_empty, true)
        .await?;

    Ok(())
}

/// Walks a subscription through its states: the hint offers to subscribe, then
/// says the verification email is on its way, then that notifications are on,
/// and finally shows nothing once the unsubscribe link has been followed.
/// Subscribing again after that is not possible yet.
async fn notifications(session: &Session) -> Result<()> {
    let s = Locale::En.strings();
    let profile = session.profile;
    let client = &session.client;
    let user = User::new(client);

    session.goto(&session.index_url()).await?;
    open_login_modal(client, profile).await?;
    log_in(client, "Alice").await?;
    book(client, &booking_day(10)?, &booking_day(12)?, profile).await?;
    assert_profile_notification_state(
        client,
        s.profile_notifications_status_none,
        s.profile_notifications_enable,
    )
    .await?;

    let subscribe = within(client, &notifications_hint(client).await?)
        .find_by_role(
            "button",
            Some(NameMatch::Contains(s.index_hint_booking_notifications_cta)),
        )
        .await?;
    user.click(&subscribe).await?;

    let email_modal = find_modal(client, Modal::Email).await?;
    let email_input = within(client, &email_modal)
        .find_by_role("textbox", Some(NameMatch::Contains(s.email_modal_name)))
        .await?;
    user.fill(&email_input, "alice@example.com").await?;
    submit(client, Modal::Email).await?;
    wait_for_modal_to_close(client, Modal::Email).await?;
    wait_for_notifications_hint(client, s.index_hint_booking_notifications_pending).await?;
    assert_profile_notification_state(
        client,
        s.profile_notifications_status_pending,
        s.profile_notifications_resend_verification,
    )
    .await?;

    session
        .goto(&wait_for_link(session, "verify").await?)
        .await?;
    session.goto(&session.index_url()).await?;
    book(client, &booking_day(20)?, &booking_day(21)?, profile).await?;
    wait_for_notifications_hint(client, s.index_hint_booking_notifications_active).await?;
    assert_profile_notification_state(
        client,
        s.profile_notifications_status_active,
        s.profile_notifications_disable,
    )
    .await?;

    // only a booking made by somebody else sends a notification, and only a
    // notification carries the unsubscribe link
    log_out(client, s.profile_login, profile).await?;
    open_login_modal(client, profile).await?;
    log_in(client, "Bob").await?;
    book(client, &booking_day(11)?, &booking_day(11)?, profile).await?;

    session
        .goto(&wait_for_link(session, "unsubscribe").await?)
        .await?;
    session.goto(&session.index_url()).await?;
    log_out(client, s.profile_login, profile).await?;
    open_login_modal(client, profile).await?;
    log_in(client, "Alice").await?;
    assert_profile_notification_state(
        client,
        s.profile_notifications_status_disabled,
        s.profile_notifications_enable,
    )
    .await?;
    book(client, &booking_day(25)?, &booking_day(26)?, profile).await?;
    wait_for_notifications_hint(client, "").await
}

/// The subscription hints sit in the bar that shows up once a booking is
/// saved.
async fn notifications_hint(client: &Client) -> Result<Element> {
    screen(client)
        .find_by_role("region", Some(NameMatch::Exact("Notifications")))
        .await
}

async fn wait_for_notifications_hint(client: &Client, text: &str) -> Result<()> {
    let hint = notifications_hint(client).await?;
    wait_for_visible_text(client, &hint, text).await
}

async fn assert_profile_notification_state(
    client: &Client,
    status: &str,
    action: &str,
) -> Result<()> {
    let profile_menu = screen(client)
        .find_by_role("button", Some(NameMatch::Exact("Profile menu")))
        .await?;
    User::new(client).click(&profile_menu).await?;
    let profile_dialog = screen(client).find_by_role("dialog", None).await?;
    wait_for_visible_text(client, &profile_dialog, status).await?;
    within(client, &profile_dialog)
        .find_by_role("button", Some(NameMatch::Exact(action)))
        .await?;
    User::new(client).click(&profile_menu).await?;
    screen(client).wait_for_role_count("dialog", None, 0).await
}

/// The notification emails go out from a task spawned while the booking is
/// saved, so their links show up shortly after.
async fn wait_for_link(session: &Session, kind: &str) -> Result<String> {
    let prefix = format!("http://{}/notifications/{kind}/", session.addr);
    let deadline = Instant::now() + WAIT_TIMEOUT;

    loop {
        let link = session
            .mailpit
            .message_bodies()
            .await?
            .iter()
            .flat_map(|body| body.split_whitespace())
            .find(|word| word.starts_with(&prefix))
            .map(str::to_owned);

        if let Some(link) = link {
            return Ok(link);
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for an email with a {kind} link");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn book(client: &Client, start: &str, end: &str, profile: BrowserProfile) -> Result<()> {
    pick_days(client, start, end, profile).await?;
    let modal = find_modal(client, Modal::Booking).await?;
    let guest_count = within(client, &modal)
        .find_by_role(
            "spinbutton",
            Some(NameMatch::Contains(
                Locale::En.strings().booking_modal_guest_count,
            )),
        )
        .await?;
    User::new(client).fill(&guest_count, "1").await?;
    submit(client, Modal::Booking).await?;
    wait_for_modal_to_close(client, Modal::Booking).await
}

async fn log_in(client: &Client, name: &str) -> Result<()> {
    log_in_as(client, name, name).await
}

async fn log_in_as(client: &Client, name: &str, expected_name: &str) -> Result<()> {
    let modal = find_modal(client, Modal::Login).await?;
    let input = within(client, &modal)
        .find_by_role(
            "textbox",
            Some(NameMatch::Contains(Locale::En.strings().name_modal_name)),
        )
        .await?;
    User::new(client).fill(&input, name).await?;
    submit(client, Modal::Login).await?;
    wait_for_modal_to_close(client, Modal::Login).await?;
    wait_for_text(client, LOGGED_IN_INFO, expected_name).await
}

async fn log_out(client: &Client, login_label: &str, profile: BrowserProfile) -> Result<()> {
    profile.dismiss_popovers(client).await?;
    let profile_menu = screen(client)
        .find_by_role("button", Some(NameMatch::Exact("Profile menu")))
        .await?;
    User::new(client).click(&profile_menu).await?;
    let disconnect = screen(client)
        .find_by_role(
            "button",
            Some(NameMatch::Exact(Locale::En.strings().profile_disconnect)),
        )
        .await?;
    User::new(client).click(&disconnect).await?;
    wait_for_text(client, LOGGED_IN_INFO, login_label).await
}

async fn open_login_modal(client: &Client, profile: BrowserProfile) -> Result<()> {
    profile.dismiss_popovers(client).await?;
    let login = screen(client)
        .find_by_role(
            "button",
            Some(NameMatch::Exact(Locale::En.strings().profile_login)),
        )
        .await?;
    User::new(client).click(&login).await
}

async fn pick_days(client: &Client, start: &str, end: &str, profile: BrowserProfile) -> Result<()> {
    let popover = open_popover(client, start, profile).await?;
    let book = within(client, &popover)
        .find_by_role(
            "button",
            Some(NameMatch::Exact(Locale::En.strings().start_booking)),
        )
        .await?;
    User::new(client).click(&book).await?;
    profile.select_end_day(client, end).await
}

async fn open_popover(client: &Client, day: &str, profile: BrowserProfile) -> Result<Element> {
    profile.dismiss_popovers(client).await?;
    let button = find_day_button(client, day).await?;
    User::new(client).click(&button).await?;
    profile.wait_for_open_popover(client, day).await
}

/// Saving a booking closes the day popover, so it has to be reopened before
/// its entries can be read.
async fn single_popover_entry(
    client: &Client,
    day: &str,
    profile: BrowserProfile,
) -> Result<String> {
    let popover = open_popover(client, day, profile).await?;
    wait_for_count_in(client, &popover, "li", 1).await?;

    let entries = texts_in(client, &popover, "li").await?;
    entries
        .into_iter()
        .next()
        .context("no booking entry in the popover")
}

async fn wait_for_booking_action_count(
    client: &Client,
    popover: &Element,
    s: &Strings,
    count: usize,
) -> Result<()> {
    let popover = within(client, popover);
    popover
        .wait_for_role_count(
            "button",
            Some(NameMatch::Exact(s.day_popover_edit_button_title)),
            count,
        )
        .await?;
    popover
        .wait_for_role_count(
            "button",
            Some(NameMatch::Exact(s.day_popover_delete_button_title)),
            count,
        )
        .await
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

async fn serve(smtp_url: &str) -> Result<SocketAddr> {
    let port = free_port()?;
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    let signed_cookie_key = vec![0u8; 64];
    let email_sender = email::SmtpSender::new(smtp_url).context("building the SMTP sender")?;

    tokio::spawn(async move {
        start(StartOptions {
            db_path: MEMORY_DB,
            listen_address: addr,
            signed_cookie_key: &signed_cookie_key,
            timezone: Some(TimeZone::UTC),
            max_capacity: 6,
            email_sender: Box::new(email_sender),
            notifications_from_address: "Bouc <bouc@example.com>",
            default_locale: Locale::En,
            base_url: &format!("http://{addr}"),
        })
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
        wait_for_port(port, "chromedriver").await?;

        Ok(driver)
    }
}

impl Drop for Chromedriver {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

async fn wait_for_port(port: u16, what: &str) -> Result<()> {
    let deadline = Instant::now() + WAIT_TIMEOUT;

    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        if Instant::now() >= deadline {
            bail!("{what} did not start listening on port {port}");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    Ok(())
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

static DAY_FORMATTER_EN: LazyLock<DateTimeFormatter<MD>> = LazyLock::new(|| {
    DateTimeFormatter::try_new(locale!("en").into(), MD::long())
        .expect("failed to build English day formatter")
});

fn accessible_day_name(day: &str) -> Result<String> {
    let date = jiff::civil::Date::strptime("%F", day).context("parsing booking day")?;
    let icu_date: IcuDate<Iso> = date.convert_into();
    Ok(DAY_FORMATTER_EN.format(&icu_date).to_string())
}

async fn find_day_button(client: &Client, day: &str) -> Result<Element> {
    screen(client)
        .find_by_role(
            "button",
            Some(NameMatch::Contains(&accessible_day_name(day)?)),
        )
        .await
}

async fn wait_for_day_name_contains(
    client: &Client,
    day: &str,
    value: &str,
    present: bool,
) -> Result<()> {
    let day_name = accessible_day_name(day)?;
    let values = [day_name.as_str(), value];
    screen(client)
        .wait_for_role_count(
            "button",
            Some(NameMatch::AllContains(&values)),
            usize::from(present),
        )
        .await
}

async fn find_modal(client: &Client, modal: Modal) -> Result<Element> {
    screen(client)
        .find_by_role(
            "dialog",
            Some(NameMatch::AnyExact(modal.accessible_names())),
        )
        .await
}

async fn submit(client: &Client, modal: Modal) -> Result<()> {
    let modal_element = find_modal(client, modal).await?;
    let save = within(client, &modal_element)
        .find_by_role("button", Some(NameMatch::Exact("Save")))
        .await?;
    User::new(client)
        .click(&save)
        .await
        .with_context(|| format!("submitting the {modal:?} modal"))
}

async fn close_modal(client: &Client, modal: &Element) -> Result<()> {
    let close = within(client, modal)
        .find_by_role("button", Some(NameMatch::Exact("Close")))
        .await?;
    User::new(client)
        .click(&close)
        .await
        .context("closing modal")
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

async fn modal_open(client: &Client, modal: Modal) -> Result<bool> {
    Ok(!screen(client)
        .query_all_by_role(
            "dialog",
            Some(NameMatch::AnyExact(modal.accessible_names())),
        )
        .await?
        .is_empty())
}

async fn wait_for_modal_to_close(client: &Client, modal: Modal) -> Result<()> {
    screen(client)
        .wait_for_role_count(
            "dialog",
            Some(NameMatch::AnyExact(modal.accessible_names())),
            0,
        )
        .await
}
