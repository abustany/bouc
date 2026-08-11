use std::sync::Arc;

use anyhow::Context;

use bouc::sqlite::SqliteRepository;
use bouc::web;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(db_filename) = args.next() else {
        anyhow::bail!("missing required command line parameter: DB filename");
    };
    let repo = SqliteRepository::open(&db_filename)
        .await
        .context("opening database")?;

    let app = web::router(Arc::new(repo));

    let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding listener on {addr}"))?;
    println!("listening on http://{}", listener.local_addr()?);

    axum::serve(listener, app).await.context("serving")?;
    Ok(())
}
