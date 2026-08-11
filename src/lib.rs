use std::{path::Path, sync::Arc};

use crate::sqlite::SqliteRepository;

pub mod bookings;
pub mod sqlite;
pub mod strings;
mod views;
pub mod web;

use anyhow::Context;
use tokio::net::ToSocketAddrs;

pub async fn start<ListenAddr: ToSocketAddrs>(
    db_path: impl AsRef<Path>,
    listen_address: ListenAddr,
) -> anyhow::Result<()> {
    let repo = SqliteRepository::open(db_path)
        .await
        .context("opening database")?;

    let router = web::router(Arc::new(repo));
    let listener = tokio::net::TcpListener::bind(listen_address)
        .await
        .context("binding listener")?;
    println!("listening on http://{}", listener.local_addr()?);

    axum::serve(listener, router).await.context("serving")
}
