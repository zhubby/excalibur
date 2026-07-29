use std::net::SocketAddr;

use anyhow::{Context, bail};
use excalibur_api::{AppState, app_with_state};
use excalibur_storage::{PgStore, Store};
use tokio::net::TcpListener;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, PartialEq, Eq)]
enum StorageConfig {
    Memory,
    Sql {
        backend: String,
        database_url: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "excalibur_api=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let store = build_store(storage_config(
        std::env::var("STORAGE_BACKEND").ok(),
        std::env::var("DATABASE_URL").ok(),
    )?)
    .await?;

    let addr: SocketAddr = std::env::var("API_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
        .parse()
        .context("API_ADDR must be a socket address")?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "starting excalibur api");
    axum::serve(listener, app_with_state(AppState::new(store))).await?;
    Ok(())
}

fn storage_config(
    storage_backend: Option<String>,
    database_url: Option<String>,
) -> anyhow::Result<StorageConfig> {
    let storage_backend = storage_backend.unwrap_or_else(|| "memory".to_owned());
    match storage_backend.as_str() {
        "memory" => Ok(StorageConfig::Memory),
        "timescale" => Ok(StorageConfig::Sql {
            backend: storage_backend,
            database_url: database_url.context("DATABASE_URL is required for SQL storage")?,
        }),
        value => {
            bail!("unsupported STORAGE_BACKEND={value}; expected memory or timescale")
        }
    }
}

async fn build_store(config: StorageConfig) -> anyhow::Result<Store> {
    let store = match config {
        StorageConfig::Memory => {
            tracing::warn!("using in-memory development store");
            Store::memory()
        }
        StorageConfig::Sql {
            backend,
            database_url,
        } => {
            let pg_store = PgStore::connect(&database_url)
                .await
                .context("failed to connect SQL storage")?;
            pg_store
                .validate_schema()
                .await
                .context("SQL schema validation failed")?;
            tracing::info!(backend, "using SQL storage");
            Store::postgres(pg_store)
        }
    };
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_config_defaults_to_memory() {
        assert_eq!(storage_config(None, None).unwrap(), StorageConfig::Memory);
    }

    #[test]
    fn storage_config_requires_database_url_for_sql() {
        let error = storage_config(Some("timescale".to_owned()), None).unwrap_err();

        assert!(error.to_string().contains("DATABASE_URL is required"));
    }

    #[test]
    fn storage_config_accepts_timescale() {
        assert_eq!(
            storage_config(
                Some("timescale".to_owned()),
                Some("postgres://example/timescale".to_owned())
            )
            .unwrap(),
            StorageConfig::Sql {
                backend: "timescale".to_owned(),
                database_url: "postgres://example/timescale".to_owned()
            }
        );
    }

    #[test]
    fn storage_config_rejects_unknown_backend() {
        let error = storage_config(Some("postgres".to_owned()), None).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("unsupported STORAGE_BACKEND=postgres")
        );
    }
}
