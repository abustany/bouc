use std::path::Path;

use crate::sqlite::SqliteRepository;

pub mod bookings;
pub mod sqlite;
pub mod strings;
mod views;
pub mod web;

use anyhow::Context;
use axum_extra::extract::cookie::Key;
use jiff::tz::TimeZone;
use tokio::net::ToSocketAddrs;

pub async fn start<ListenAddr: ToSocketAddrs>(
    db_path: impl AsRef<Path>,
    listen_address: ListenAddr,
    signed_cookie_key: &[u8],
    timezone: Option<TimeZone>,
    max_capacity: u32,
) -> anyhow::Result<()> {
    let signed_cookies_key =
        Key::try_from(signed_cookie_key).context("validating cookie signing key")?;

    let repo = SqliteRepository::open(db_path)
        .await
        .context("opening database")?;

    let router = web::router(
        repo,
        signed_cookies_key,
        timezone.unwrap_or_else(TimeZone::system),
        max_capacity,
    );
    let listener = tokio::net::TcpListener::bind(listen_address)
        .await
        .context("binding listener")?;
    println!("listening on http://{}", listener.local_addr()?);

    axum::serve(listener, router).await.context("serving")
}
