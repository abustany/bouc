use anyhow::Context;

use bouc::start;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(db_filename) = args.next() else {
        anyhow::bail!("missing required command line parameter: DB filename");
    };
    let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
    start(&db_filename, &addr).await.context("starting app")?;
    Ok(())
}
