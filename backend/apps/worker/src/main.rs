use std::time::Duration;

use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "excalibur_worker=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("starting excalibur worker");
    loop {
        tracing::debug!(
            "worker heartbeat: alert scans, action timeouts, and retention jobs attach here"
        );
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
