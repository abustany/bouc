use std::path::Path;
use std::sync::Arc;

use crate::{bookings::Repository, notify::Notifier, sqlite::SqliteRepository};

pub mod bookings;
pub mod email;
mod interpolate;
pub mod notify;
pub mod sqlite;
pub mod strings;
mod views;
pub mod web;

use anyhow::Context;
use axum_extra::extract::cookie::Key;
use jiff::tz::TimeZone;
use tokio::net::ToSocketAddrs;

pub struct StartOptions<'a, 'b, 'c, DbPath: AsRef<Path>, ListenAddr: ToSocketAddrs> {
    pub db_path: DbPath,
    pub listen_address: ListenAddr,
    pub signed_cookie_key: &'a [u8],
    pub timezone: Option<TimeZone>,
    pub max_capacity: u32,
    pub email_sender: Box<dyn email::Sender>,
    pub notifications_from_address: &'b str,
    pub default_locale: strings::Locale,
    pub base_url: &'c str,
}

pub async fn start<'a, 'b, 'c, DbPath: AsRef<Path>, ListenAddr: ToSocketAddrs>(
    opts: StartOptions<'a, 'b, 'c, DbPath, ListenAddr>,
) -> anyhow::Result<()> {
    let signed_cookies_key =
        Key::try_from(opts.signed_cookie_key).context("validating cookie signing key")?;

    let repo: Arc<dyn Repository> = Arc::new(
        SqliteRepository::open(opts.db_path)
            .await
            .context("opening database")?,
    );

    let notifier = Arc::new(Notifier::new(
        repo.clone(),
        opts.email_sender,
        opts.notifications_from_address,
        opts.default_locale,
        opts.base_url,
    ));

    let router = web::router(
        repo,
        signed_cookies_key,
        opts.timezone.unwrap_or_else(TimeZone::system),
        opts.max_capacity,
        notifier,
    );
    let listener = tokio::net::TcpListener::bind(opts.listen_address)
        .await
        .context("binding listener")?;
    println!("listening on http://{}", listener.local_addr()?);

    axum::serve(listener, router).await.context("serving")
}
