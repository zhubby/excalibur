use std::{
    collections::HashMap,
    net::SocketAddr,
    str,
    thread::{self, JoinHandle},
};

use anyhow::{Context, bail};
use excalibur_device_protocol::{PublishTopic, parse_publish_topic};
use excalibur_domain::{DeviceStatus, Id};
use excalibur_mqtt_ingest::{AuthenticatedDevice, ingest_publish};
use excalibur_storage::{PgStore, Store};
use rumqttd::{Broker, Config, ConnectionSettings, Notification, RouterConfig, ServerSettings};
use serde_json::Value;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

const INGEST_FILTERS: &[&str] = &[
    "v1/p/+/d/+/telemetry/+",
    "v1/p/+/d/+/shadow",
    "v1/p/+/d/+/commands/status",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum StorageConfig {
    Memory,
    Sql {
        backend: String,
        database_url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MqttRuntimeConfig {
    listen: SocketAddr,
    max_connections: usize,
    max_payload_size: usize,
    max_inflight_count: usize,
    connection_timeout_ms: u16,
    router_max_outgoing_packet_count: u64,
    router_max_segment_size: usize,
    router_max_segment_count: usize,
    storage: StorageConfig,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "excalibur_mqtt_ingest=info,rumqttd=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let runtime = runtime_config_from_env()?;
    info!("building MQTT ingest store");
    let store = build_store(runtime.storage.clone()).await?;
    info!("checking MQTT ingest store health");
    store.health_check().await?;
    info!("MQTT ingest store ready");

    info!(listen = %runtime.listen, "initializing rumqttd broker");
    let broker = Broker::new(rumqttd_config(&runtime));
    info!("creating rumqttd local ingest link");
    let (mut link_tx, link_rx) = broker
        .link("excalibur-mqtt-ingest")
        .context("failed to create rumqttd local ingest link")?;
    for filter in INGEST_FILTERS {
        link_tx
            .subscribe(*filter)
            .with_context(|| format!("failed to subscribe ingest link to {filter}"))?;
        info!(filter, "subscribed ingest link");
    }

    let _broker_thread = start_broker_thread(broker);
    info!(
        listen = %runtime.listen,
        filters = ?INGEST_FILTERS,
        "excalibur mqtt broker and ingest runtime started"
    );

    tokio::select! {
        result = run_ingest_loop(link_rx, store) => result,
        result = tokio::signal::ctrl_c() => {
            result.context("failed to wait for shutdown signal")?;
            info!("shutdown signal received");
            Ok(())
        }
    }
}

async fn run_ingest_loop(mut link_rx: rumqttd::local::LinkRx, store: Store) -> anyhow::Result<()> {
    loop {
        let Some(notification) = link_rx
            .next()
            .await
            .context("rumqttd ingest link receive failed")?
        else {
            continue;
        };

        let Notification::Forward(forward) = notification else {
            continue;
        };

        let topic = match str::from_utf8(&forward.publish.topic) {
            Ok(topic) => topic,
            Err(error) => {
                warn!(%error, "dropping MQTT publish with non-UTF-8 topic");
                continue;
            }
        };

        let payload = match serde_json::from_slice::<Value>(&forward.publish.payload) {
            Ok(payload) => payload,
            Err(error) => {
                warn!(%topic, %error, "dropping MQTT publish with invalid JSON payload");
                continue;
            }
        };

        let device = match authenticated_device_for_topic(&store, topic).await {
            Ok(device) => device,
            Err(error) => {
                warn!(%topic, %error, "dropping MQTT publish for unknown or unauthorized device");
                continue;
            }
        };

        match ingest_publish(&store, topic, payload, device).await {
            Ok(count) => {
                info!(%topic, count, "ingested MQTT publish");
            }
            Err(error) => {
                warn!(%topic, %error, "failed to ingest MQTT publish");
            }
        }
    }
}

async fn authenticated_device_for_topic(
    store: &Store,
    topic: &str,
) -> anyhow::Result<AuthenticatedDevice> {
    let parsed = parse_publish_topic(topic)?;
    let (project_id, device_id) = topic_scope(&parsed);
    let device = store.get_device(project_id, device_id).await?;
    if matches!(device.status, DeviceStatus::Disabled) {
        bail!("device is disabled");
    }

    Ok(AuthenticatedDevice {
        project_id,
        device_id,
        status: device.status,
    })
}

fn topic_scope(topic: &PublishTopic) -> (Id, Id) {
    match topic {
        PublishTopic::Telemetry {
            project_id,
            device_id,
            ..
        }
        | PublishTopic::Shadow {
            project_id,
            device_id,
        }
        | PublishTopic::CommandStatus {
            project_id,
            device_id,
        } => (*project_id, *device_id),
    }
}

fn start_broker_thread(mut broker: Broker) -> JoinHandle<()> {
    thread::Builder::new()
        .name("rumqttd-broker".to_owned())
        .spawn(move || {
            if let Err(error) = broker.start() {
                error!(?error, "rumqttd broker stopped");
            }
        })
        .expect("failed to spawn rumqttd broker thread")
}

fn runtime_config_from_env() -> anyhow::Result<MqttRuntimeConfig> {
    let database_url = std::env::var("DATABASE_URL").ok();
    let storage_backend = std::env::var("STORAGE_BACKEND").ok();

    Ok(MqttRuntimeConfig {
        listen: parse_env("MQTT_LISTEN", "0.0.0.0:1883")?,
        max_connections: parse_env("MQTT_MAX_CONNECTIONS", "10000")?,
        max_payload_size: parse_env("MQTT_MAX_PAYLOAD_SIZE", "262144")?,
        max_inflight_count: parse_env("MQTT_MAX_INFLIGHT_COUNT", "100")?,
        connection_timeout_ms: parse_env("MQTT_CONNECTION_TIMEOUT_MS", "60000")?,
        router_max_outgoing_packet_count: parse_env(
            "MQTT_ROUTER_MAX_OUTGOING_PACKET_COUNT",
            "200",
        )?,
        router_max_segment_size: parse_env("MQTT_ROUTER_MAX_SEGMENT_SIZE", "104857600")?,
        router_max_segment_count: parse_env("MQTT_ROUTER_MAX_SEGMENT_COUNT", "10")?,
        storage: storage_config(storage_backend, database_url)?,
    })
}

fn parse_env<T>(name: &str, default: &str) -> anyhow::Result<T>
where
    T: str::FromStr,
    T::Err: std::fmt::Display,
{
    std::env::var(name)
        .unwrap_or_else(|_| default.to_owned())
        .parse()
        .map_err(|error| anyhow::anyhow!("{name} is invalid: {error}"))
}

fn storage_config(
    storage_backend: Option<String>,
    database_url: Option<String>,
) -> anyhow::Result<StorageConfig> {
    let storage_backend = storage_backend.unwrap_or_else(|| {
        if database_url.is_some() {
            "timescale"
        } else {
            "memory"
        }
        .to_owned()
    });
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
    match config {
        StorageConfig::Memory => {
            warn!("using in-memory MQTT ingest store");
            Ok(Store::memory())
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
            info!(backend, "using SQL storage for MQTT ingest");
            Ok(Store::postgres(pg_store))
        }
    }
}

fn rumqttd_config(runtime: &MqttRuntimeConfig) -> Config {
    let mut v4 = HashMap::new();
    v4.insert(
        "excalibur".to_owned(),
        ServerSettings {
            name: "excalibur-mqtt-v4".to_owned(),
            listen: runtime.listen,
            tls: None,
            next_connection_delay_ms: 1,
            connections: ConnectionSettings {
                connection_timeout_ms: runtime.connection_timeout_ms,
                max_payload_size: runtime.max_payload_size,
                max_inflight_count: runtime.max_inflight_count,
                auth: None,
                external_auth: None,
                dynamic_filters: true,
            },
        },
    );

    Config {
        id: 0,
        router: RouterConfig {
            max_connections: runtime.max_connections,
            max_outgoing_packet_count: runtime.router_max_outgoing_packet_count,
            max_segment_size: runtime.router_max_segment_size,
            max_segment_count: runtime.router_max_segment_count,
            custom_segment: None,
            initialized_filters: Some(
                INGEST_FILTERS
                    .iter()
                    .map(|filter| (*filter).to_owned())
                    .collect(),
            ),
            shared_subscriptions_strategy: Default::default(),
        },
        v4: Some(v4),
        v5: None,
        ws: None,
        cluster: None,
        console: None,
        bridge: None,
        prometheus: None,
        metrics: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_config_defaults_to_memory_without_database_url() {
        assert_eq!(storage_config(None, None).unwrap(), StorageConfig::Memory);
    }

    #[test]
    fn storage_config_defaults_to_timescale_when_database_url_exists() {
        assert_eq!(
            storage_config(None, Some("postgres://example/excalibur".to_owned())).unwrap(),
            StorageConfig::Sql {
                backend: "timescale".to_owned(),
                database_url: "postgres://example/excalibur".to_owned()
            }
        );
    }

    #[test]
    fn storage_config_requires_database_url_for_explicit_timescale() {
        let error = storage_config(Some("timescale".to_owned()), None).unwrap_err();

        assert!(error.to_string().contains("DATABASE_URL is required"));
    }

    #[test]
    fn rumqttd_config_exposes_v4_listener_and_ingest_filters() {
        let runtime = MqttRuntimeConfig {
            listen: "127.0.0.1:18830".parse().unwrap(),
            max_connections: 10,
            max_payload_size: 4096,
            max_inflight_count: 8,
            connection_timeout_ms: 1000,
            router_max_outgoing_packet_count: 32,
            router_max_segment_size: 1024,
            router_max_segment_count: 2,
            storage: StorageConfig::Memory,
        };

        let config = rumqttd_config(&runtime);
        let server = config.v4.as_ref().unwrap().get("excalibur").unwrap();

        assert_eq!(server.listen, runtime.listen);
        assert_eq!(server.connections.max_payload_size, 4096);
        assert_eq!(
            config.router.initialized_filters.unwrap(),
            vec![
                "v1/p/+/d/+/telemetry/+".to_owned(),
                "v1/p/+/d/+/shadow".to_owned(),
                "v1/p/+/d/+/commands/status".to_owned(),
            ]
        );
    }
}
