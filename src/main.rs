use anyhow::Context;

use base64::prelude::*;
use bouc::start;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let signed_cookie_key = BASE64_STANDARD
        .decode(&args.signed_cookies_key)
        .context("decoding cookie signing key")?;

    start(
        &args.db,
        &args.listen,
        &signed_cookie_key,
        None,
        args.max_capacity,
    )
    .await
    .context("starting app")?;
    Ok(())
}
