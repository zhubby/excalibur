use std::net::SocketAddr;

use anyhow::{Context, bail};
use excalibur_api::app;
use tokio::net::TcpListener;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "excalibur_api=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let storage_backend = std::env::var("STORAGE_BACKEND").unwrap_or_else(|_| "memory".to_owned());
    match storage_backend.as_str() {
        "memory" => tracing::warn!(
            "using in-memory development store; set STORAGE_BACKEND once SQL repositories are implemented"
        ),
        "postgres" | "timescale" => bail!(
            "STORAGE_BACKEND={storage_backend} requested, but SQL repositories are not implemented yet"
        ),
        value => {
            bail!("unsupported STORAGE_BACKEND={value}; expected memory, postgres, or timescale")
        }
    }

    let addr: SocketAddr = std::env::var("API_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
        .parse()
        .context("API_ADDR must be a socket address")?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "starting excalibur api");
    axum::serve(listener, app()).await?;
    Ok(())
}
