use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "excalibur_mqtt_ingest=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!(
        "excalibur mqtt ingest adapter ready; enable the rumqttd-runtime feature for broker hook wiring"
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}
