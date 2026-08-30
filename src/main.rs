use anyhow::{Context, bail};

use base64::prelude::*;
use bouc::{StartOptions, email, start, strings::Locale};
use clap::Parser;

#[derive(Debug, Parser)]
struct Args {
    /// Path to the SQLite database where bookings are saved
    db: String,

    /// Address to listen on for incoming connections
    #[clap(default_value = "127.0.0.1:3000")]
    #[arg(long)]
    listen: String,

    /// Key used to sign cookies, at least 64 bytes, base64 encoded
    ///
    /// The key may additionally be provided in the SIGNED_COOKIES_KEY
    /// environment variable.
    #[clap(long)]
    signed_cookies_key: Option<String>,

    /// Maximum occupancy of the house
    #[clap(long)]
    max_capacity: u32,

    /// Address to use as the sender of notifications
    #[clap(long)]
    notifications_sender_address: String,

    /// Default locale to use for web UI and notifications
    #[clap(long)]
    default_locale: String,

    /// Public base URL, if different from the listen address
    #[clap(long)]
    base_url: Option<String>,

    /// URL of the SMTP server to use for sending notifications
    ///
    /// Format: smtp[s]://username:password@host:port
    ///
    /// The password may additionally be provided in the SMTP_PASSWORD
    /// environment variable.
    #[clap(long)]
    smtp_server_url: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let signed_cookie_key = BASE64_STANDARD
        .decode(
            args.signed_cookies_key
                .or_else(|| std::env::var("SIGNED_COOKIES_KEY").ok())
                .context("no cookie signing key defined")?,
        )
        .context("decoding cookie signing key")?;

    let email_sender: Box<dyn email::Sender> = if let Some(url) = args.smtp_server_url {
        let mut parsed_url: url::Url = url.parse().context("parsing SMTP server URL")?;
        if let Ok(password) = std::env::var("SMTP_PASSWORD")
            && parsed_url.set_password(Some(&password)).is_err() {
                bail!("error setting SMTP url password");
            }

        Box::new(email::SmtpSender::new(parsed_url.as_str()).context("building SMTP sender")?)
    } else {
        Box::new(email::ConsoleSender::new())
    };

    start(StartOptions {
        db_path: &args.db,
        listen_address: &args.listen,
        signed_cookie_key: &signed_cookie_key,
        timezone: None,
        max_capacity: args.max_capacity,
        email_sender,
        notifications_from_address: &args.notifications_sender_address,
        default_locale: Locale::from_language_tag(&args.default_locale)
            .context("unsupported locale")?,
        base_url: &args
            .base_url
            .unwrap_or_else(|| format!("http://{}", args.listen)),
    })
    .await
    .context("starting app")?;
    Ok(())
}
