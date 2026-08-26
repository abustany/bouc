use anyhow::Context;

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
    #[clap(long)]
    signed_cookies_key: String,

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let signed_cookie_key = BASE64_STANDARD
        .decode(&args.signed_cookies_key)
        .context("decoding cookie signing key")?;

    start(StartOptions {
        db_path: &args.db,
        listen_address: &args.listen,
        signed_cookie_key: &signed_cookie_key,
        timezone: None,
        max_capacity: args.max_capacity,
        email_sender: Box::new(email::ConsoleSender::new()),
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
