use std::{
    collections::HashMap,
    net::SocketAddr,
    str,
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use anyhow::{Context, bail};
#[cfg(test)]
use chrono::Utc;
use excalibur_device_protocol::{
    DeviceCommandEnvelope, PublishTopic, TelemetryIngestEnvelope, commands_topic,
    parse_publish_topic,
};
use excalibur_domain::{DeviceStatus, Id};
use excalibur_mqtt_ingest::{
    AuthenticatedDevice, authenticate_device_certificate_fingerprint, ingest_publish,
    telemetry_envelope_from_publish,
};
use excalibur_nats_lite::NatsClient;
use excalibur_storage::{PgStore, Store};
use rumqttd::{
    Broker, Config, ConnectionSettings, Notification, RouterConfig, ServerSettings, TlsConfig,
};
use serde_json::{Value, json};
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
    tls: Option<MqttTlsConfig>,
    require_cert_fingerprint_username: bool,
    telemetry_buffer: TelemetryBufferConfig,
    command_bridge: CommandBridgeConfig,
    storage: StorageConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MqttTlsConfig {
    ca_cert_path: String,
    server_cert_path: String,
    server_key_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TelemetryBufferConfig {
    Direct,
    Nats {
        url: String,
        subject: String,
        stream: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommandBridgeConfig {
    Disabled,
    Nats { url: String, subject: String },
}

#[derive(Debug, Clone)]
struct MqttClientIdentity {
    fingerprint_sha256: String,
    device: AuthenticatedDevice,
}

type ClientIdentityRegistry = Arc<RwLock<HashMap<String, MqttClientIdentity>>>;

#[derive(Debug, Clone)]
enum TelemetrySink {
    Direct,
    Nats {
        client: NatsClient,
        subject: String,
        stream: String,
    },
}

impl TelemetrySink {
    fn from_config(config: &TelemetryBufferConfig) -> anyhow::Result<Self> {
        match config {
            TelemetryBufferConfig::Direct => Ok(Self::Direct),
            TelemetryBufferConfig::Nats {
                url,
                subject,
                stream,
            } => Ok(Self::Nats {
                client: NatsClient::new(url, "excalibur-mqtt-ingest")?,
                subject: subject.clone(),
                stream: stream.clone(),
            }),
        }
    }
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
    let telemetry_sink = TelemetrySink::from_config(&runtime.telemetry_buffer)?;
    ensure_telemetry_stream(&telemetry_sink).await?;
    let client_identities = Arc::new(RwLock::new(HashMap::new()));

    info!(listen = %runtime.listen, "initializing rumqttd broker");
    let broker = Broker::new(rumqttd_config(
        &runtime,
        store.clone(),
        telemetry_sink.clone(),
        client_identities.clone(),
    ));
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
    let command_bridge_task = start_command_bridge_task(link_tx, runtime.command_bridge.clone());
    info!(
        listen = %runtime.listen,
        filters = ?INGEST_FILTERS,
        "excalibur mqtt broker and ingest runtime started"
    );

    if let Some(command_bridge_task) = command_bridge_task {
        tokio::select! {
            result = run_ingest_loop(
                link_rx,
                store,
                telemetry_sink,
                client_identities,
                runtime.require_cert_fingerprint_username,
            ) => result,
            result = command_bridge_task => {
                result.context("MQTT command bridge task panicked")?
            },
            result = tokio::signal::ctrl_c() => {
                result.context("failed to wait for shutdown signal")?;
                info!("shutdown signal received");
                Ok(())
            }
        }
    } else {
        tokio::select! {
            result = run_ingest_loop(
                link_rx,
                store,
                telemetry_sink,
                client_identities,
                runtime.require_cert_fingerprint_username,
            ) => result,
            result = tokio::signal::ctrl_c() => {
                result.context("failed to wait for shutdown signal")?;
                info!("shutdown signal received");
                Ok(())
            }
        }
    }
}

async fn run_ingest_loop(
    mut link_rx: rumqttd::local::LinkRx,
    store: Store,
    telemetry_sink: TelemetrySink,
    client_identities: ClientIdentityRegistry,
    require_connection_identity: bool,
) -> anyhow::Result<()> {
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

        let device = match authenticated_device_for_forward(
            &store,
            &client_identities,
            forward.source_client_id.as_deref(),
            topic,
            require_connection_identity,
        )
        .await
        {
            Ok(device) => device,
            Err(error) => {
                warn!(%topic, %error, "dropping MQTT publish for unknown or unauthorized device");
                continue;
            }
        };

        match handle_ingest_publish(&store, &telemetry_sink, topic, payload, device).await {
            Ok(IngestPublishOutcome::Written(count)) => {
                info!(%topic, count, "ingested MQTT publish");
            }
            Ok(IngestPublishOutcome::DurablyAcceptedUpstream) => {
                info!(%topic, "MQTT telemetry already durably accepted before broker ack");
            }
            Err(error) => {
                warn!(%topic, %error, "failed to ingest MQTT publish");
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IngestPublishOutcome {
    Written(usize),
    DurablyAcceptedUpstream,
}

async fn handle_ingest_publish(
    store: &Store,
    telemetry_sink: &TelemetrySink,
    topic: &str,
    payload: Value,
    device: AuthenticatedDevice,
) -> anyhow::Result<IngestPublishOutcome> {
    let parsed = parse_publish_topic(topic)?;
    if matches!(parsed, PublishTopic::Telemetry { .. }) && telemetry_sink.remote_ack_gate_enabled()
    {
        return Ok(IngestPublishOutcome::DurablyAcceptedUpstream);
    }
    match (&parsed, telemetry_sink) {
        (PublishTopic::Telemetry { .. }, TelemetrySink::Nats { .. }) => {
            let envelope = telemetry_envelope_from_publish(topic, payload, device)?;
            publish_telemetry_envelope_to_jetstream(telemetry_sink, envelope)
                .await
                .map(IngestPublishOutcome::Written)
        }
        _ => ingest_publish(store, topic, payload, device)
            .await
            .map(IngestPublishOutcome::Written)
            .map_err(anyhow::Error::from),
    }
}

impl TelemetrySink {
    fn remote_ack_gate_enabled(&self) -> bool {
        matches!(self, TelemetrySink::Nats { .. })
    }
}

async fn durably_accept_remote_publish(
    store: &Store,
    telemetry_sink: &TelemetrySink,
    client_identities: &ClientIdentityRegistry,
    source_client_id: Option<&str>,
    topic: &str,
    payload: &[u8],
    require_connection_identity: bool,
) -> anyhow::Result<Option<usize>> {
    let parsed = match parse_publish_topic(topic) {
        Ok(parsed) => parsed,
        Err(error) if is_excalibur_publish_namespace(topic) => {
            bail!("invalid Excalibur publish topic: {error}");
        }
        Err(_) => return Ok(None),
    };
    if !matches!(parsed, PublishTopic::Telemetry { .. }) {
        return Ok(None);
    }
    let payload =
        serde_json::from_slice::<Value>(payload).context("telemetry payload is not valid JSON")?;
    let device = authenticated_device_for_forward(
        store,
        client_identities,
        source_client_id,
        topic,
        require_connection_identity,
    )
    .await?;
    let envelope = telemetry_envelope_from_publish(topic, payload, device)?;
    publish_telemetry_envelope_to_jetstream(telemetry_sink, envelope)
        .await
        .map(Some)
}

fn is_excalibur_publish_namespace(topic: &str) -> bool {
    let topic = topic.trim_matches('/');
    topic.is_empty() || topic == "v1" || topic.starts_with("v1/")
}

async fn publish_telemetry_envelope_to_jetstream(
    telemetry_sink: &TelemetrySink,
    envelope: TelemetryIngestEnvelope,
) -> anyhow::Result<usize> {
    let TelemetrySink::Nats {
        client,
        subject,
        stream,
    } = telemetry_sink
    else {
        bail!("telemetry JetStream sink is not configured");
    };
    let point_count = envelope.points.len();
    let payload =
        serde_json::to_vec(&envelope).context("failed to encode telemetry ingest envelope")?;
    let ack = client
        .publish_jetstream(subject, &payload, Duration::from_secs(5))
        .await
        .with_context(|| format!("failed to publish telemetry envelope to {subject}"))?;
    if ack.stream != *stream {
        bail!(
            "JetStream publish ack stream mismatch: expected {}, got {}",
            stream,
            ack.stream
        );
    }
    Ok(point_count)
}

async fn ensure_telemetry_stream(telemetry_sink: &TelemetrySink) -> anyhow::Result<()> {
    let TelemetrySink::Nats {
        client,
        subject,
        stream,
    } = telemetry_sink
    else {
        return Ok(());
    };
    let payload = json!({
        "name": stream,
        "subjects": [subject],
        "retention": "limits",
        "storage": "file",
        "discard": "old",
        "max_msgs": -1,
        "max_bytes": -1,
        "max_age": 0,
        "max_msg_size": -1
    });
    let api_subject = format!("$JS.API.STREAM.CREATE.{stream}");
    let response = client
        .request(
            &api_subject,
            payload.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .await
        .with_context(|| format!("failed to ensure JetStream stream {stream}"))?;
    let response_json = serde_json::from_slice::<Value>(&response.payload)
        .context("JetStream stream create response was not JSON")?;
    if let Some(error) = response_json.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let description = error
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("unknown JetStream error");
        if code != 400 || !description.contains("already in use") {
            bail!("JetStream stream create failed: {description}");
        }
    }
    verify_telemetry_stream_subject(client, stream, subject).await?;
    info!(stream, subject, "JetStream telemetry stream ready");
    Ok(())
}

async fn verify_telemetry_stream_subject(
    client: &NatsClient,
    stream: &str,
    subject: &str,
) -> anyhow::Result<()> {
    let response = client
        .request(
            &format!("$JS.API.STREAM.INFO.{stream}"),
            b"{}",
            Duration::from_secs(5),
        )
        .await
        .with_context(|| format!("failed to read JetStream stream info for {stream}"))?;
    let response_json = serde_json::from_slice::<Value>(&response.payload)
        .context("JetStream stream info response was not JSON")?;
    if let Some(error) = response_json.get("error") {
        let description = error
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("unknown JetStream error");
        bail!("JetStream stream info failed: {description}");
    }
    let subjects = response_json
        .pointer("/config/subjects")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("JetStream stream info missing config.subjects"))?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if !subjects.iter().any(|existing| existing == &subject) {
        bail!("JetStream stream {stream} is missing expected subject {subject}");
    }
    Ok(())
}

fn start_command_bridge_task(
    link_tx: rumqttd::local::LinkTx,
    command_bridge: CommandBridgeConfig,
) -> Option<tokio::task::JoinHandle<anyhow::Result<()>>> {
    let CommandBridgeConfig::Nats { url, subject } = command_bridge else {
        info!("MQTT command bridge disabled");
        return None;
    };
    Some(tokio::spawn(async move {
        run_command_bridge_loop(link_tx, url, subject).await
    }))
}

async fn run_command_bridge_loop(
    mut link_tx: rumqttd::local::LinkTx,
    nats_url: String,
    subject: String,
) -> anyhow::Result<()> {
    loop {
        if let Err(error) = run_command_bridge_subscription(&mut link_tx, &nats_url, &subject).await
        {
            warn!(%error, subject, "MQTT command bridge disconnected; retrying");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}

async fn run_command_bridge_subscription(
    link_tx: &mut rumqttd::local::LinkTx,
    nats_url: &str,
    subject: &str,
) -> anyhow::Result<()> {
    let client = NatsClient::new(nats_url, "excalibur-mqtt-command-bridge")?;
    let mut subscription = client
        .subscribe(subject, None)
        .await
        .with_context(|| format!("failed to subscribe command bridge to {subject}"))?;
    info!(subject, "MQTT command bridge ready");

    loop {
        let message = subscription.next_message().await?;
        match mqtt_command_publish_from_nats_payload(&message.payload) {
            Ok((topic, payload)) => {
                link_tx
                    .publish(topic.clone(), payload)
                    .with_context(|| format!("failed to publish command to MQTT topic {topic}"))?;
                info!(%topic, "published action command to MQTT broker");
            }
            Err(error) => {
                warn!(%error, "dropping invalid command bridge message");
            }
        }
    }
}

fn mqtt_command_publish_from_nats_payload(payload: &[u8]) -> anyhow::Result<(String, Vec<u8>)> {
    let envelope = serde_json::from_slice::<DeviceCommandEnvelope>(payload)
        .context("command bridge payload is not a DeviceCommandEnvelope")?;
    let expected_topic = commands_topic(envelope.project_id, envelope.device_id);
    if envelope.topic != expected_topic {
        bail!("command envelope topic does not match project/device identity");
    }
    let payload =
        serde_json::to_vec(&envelope.command).context("failed to encode device command payload")?;
    Ok((envelope.topic, payload))
}

async fn authenticated_device_for_forward(
    store: &Store,
    client_identities: &ClientIdentityRegistry,
    source_client_id: Option<&str>,
    topic: &str,
    require_connection_identity: bool,
) -> anyhow::Result<AuthenticatedDevice> {
    if let Some(source_client_id) = source_client_id
        && let Some(identity) = lookup_client_identity(client_identities, source_client_id)?
    {
        let device =
            authenticate_device_certificate_fingerprint(store, &identity.fingerprint_sha256)
                .await
                .context("authenticated MQTT certificate is no longer active")?;
        return Ok(device);
    }

    if require_connection_identity {
        bail!("MQTT publish is missing authenticated connection identity");
    }

    authenticated_device_for_topic(store, topic).await
}

fn lookup_client_identity(
    client_identities: &ClientIdentityRegistry,
    source_client_id: &str,
) -> anyhow::Result<Option<MqttClientIdentity>> {
    let identities = client_identities
        .read()
        .map_err(|_| anyhow::anyhow!("MQTT identity registry lock is poisoned"))?;
    if let Some(identity) = identities.get(source_client_id) {
        return Ok(Some(identity.clone()));
    }
    if let Some((_, raw_client_id)) = source_client_id.split_once('.')
        && let Some(identity) = identities.get(raw_client_id)
    {
        return Ok(Some(identity.clone()));
    }
    Ok(None)
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

fn start_broker_thread(mut broker: Broker) -> thread::JoinHandle<()> {
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
    let tls = mqtt_tls_config_from_env()?;
    let require_cert_fingerprint_username =
        parse_bool_env("MQTT_REQUIRE_CERT_FINGERPRINT_USERNAME", false)?;
    let allow_plaintext_fingerprint_auth =
        parse_bool_env("MQTT_ALLOW_PLAINTEXT_FINGERPRINT_AUTH", false)?;
    validate_mqtt_identity_binding(
        tls.is_some(),
        require_cert_fingerprint_username,
        allow_plaintext_fingerprint_auth,
    )?;

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
        tls,
        require_cert_fingerprint_username,
        telemetry_buffer: telemetry_buffer_config_from_env()?,
        command_bridge: command_bridge_config_from_env()?,
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

fn telemetry_buffer_config_from_env() -> anyhow::Result<TelemetryBufferConfig> {
    let subject = std::env::var("MQTT_TELEMETRY_NATS_SUBJECT")
        .unwrap_or_else(|_| "excalibur.telemetry.ingest".to_owned());
    let stream = std::env::var("MQTT_TELEMETRY_NATS_STREAM")
        .unwrap_or_else(|_| "EXCALIBUR_TELEMETRY".to_owned());
    match std::env::var("MQTT_TELEMETRY_BUFFER")
        .unwrap_or_else(|_| "auto".to_owned())
        .as_str()
    {
        "direct" => Ok(TelemetryBufferConfig::Direct),
        "nats" => Ok(TelemetryBufferConfig::Nats {
            url: std::env::var("NATS_URL").context("NATS_URL is required for nats buffer")?,
            subject,
            stream,
        }),
        "auto" => match std::env::var("NATS_URL") {
            Ok(url) => Ok(TelemetryBufferConfig::Nats {
                url,
                subject,
                stream,
            }),
            Err(_) => Ok(TelemetryBufferConfig::Direct),
        },
        value => bail!("unsupported MQTT_TELEMETRY_BUFFER={value}; expected auto, direct, or nats"),
    }
}

fn command_bridge_config_from_env() -> anyhow::Result<CommandBridgeConfig> {
    let subject = std::env::var("MQTT_COMMAND_NATS_SUBJECT")
        .unwrap_or_else(|_| "excalibur.commands.dispatch".to_owned());
    match std::env::var("MQTT_COMMAND_BRIDGE")
        .unwrap_or_else(|_| "auto".to_owned())
        .as_str()
    {
        "disabled" => Ok(CommandBridgeConfig::Disabled),
        "nats" => Ok(CommandBridgeConfig::Nats {
            url: std::env::var("NATS_URL").context("NATS_URL is required for command bridge")?,
            subject,
        }),
        "auto" => match std::env::var("NATS_URL") {
            Ok(url) => Ok(CommandBridgeConfig::Nats { url, subject }),
            Err(_) => Ok(CommandBridgeConfig::Disabled),
        },
        value => bail!("unsupported MQTT_COMMAND_BRIDGE={value}; expected auto, disabled, or nats"),
    }
}

fn validate_mqtt_identity_binding(
    tls_present: bool,
    require_cert_fingerprint_username: bool,
    allow_plaintext_fingerprint_auth: bool,
) -> anyhow::Result<()> {
    if require_cert_fingerprint_username && !tls_present && !allow_plaintext_fingerprint_auth {
        bail!(
            "MQTT_REQUIRE_CERT_FINGERPRINT_USERNAME requires MQTT TLS; set MQTT_ALLOW_PLAINTEXT_FINGERPRINT_AUTH=true only for local development"
        );
    }
    Ok(())
}

fn parse_bool_env(name: &str, default: bool) -> anyhow::Result<bool> {
    match std::env::var(name) {
        Ok(value) => match value.as_str() {
            "true" | "1" | "yes" | "on" => Ok(true),
            "false" | "0" | "no" | "off" => Ok(false),
            _ => bail!("{name} is invalid: expected true or false"),
        },
        Err(_) => Ok(default),
    }
}

fn mqtt_tls_config_from_env() -> anyhow::Result<Option<MqttTlsConfig>> {
    let ca_cert_path = std::env::var("MQTT_TLS_CA_CERT_PATH").ok();
    let server_cert_path = std::env::var("MQTT_TLS_SERVER_CERT_PATH").ok();
    let server_key_path = std::env::var("MQTT_TLS_SERVER_KEY_PATH").ok();
    match (ca_cert_path, server_cert_path, server_key_path) {
        (None, None, None) => Ok(None),
        (Some(ca_cert_path), Some(server_cert_path), Some(server_key_path)) => {
            Ok(Some(MqttTlsConfig {
                ca_cert_path,
                server_cert_path,
                server_key_path,
            }))
        }
        _ => bail!(
            "MQTT TLS requires MQTT_TLS_CA_CERT_PATH, MQTT_TLS_SERVER_CERT_PATH, and MQTT_TLS_SERVER_KEY_PATH together"
        ),
    }
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

fn rumqttd_config(
    runtime: &MqttRuntimeConfig,
    store: Store,
    telemetry_sink: TelemetrySink,
    client_identities: ClientIdentityRegistry,
) -> Config {
    let mut v4 = HashMap::new();
    let mut server = ServerSettings {
        name: "excalibur-mqtt-v4".to_owned(),
        listen: runtime.listen,
        tls: runtime.tls.as_ref().map(|tls| TlsConfig::Rustls {
            capath: Some(tls.ca_cert_path.clone()),
            certpath: tls.server_cert_path.clone(),
            keypath: tls.server_key_path.clone(),
        }),
        next_connection_delay_ms: 1,
        connections: ConnectionSettings {
            connection_timeout_ms: runtime.connection_timeout_ms,
            max_payload_size: runtime.max_payload_size,
            max_inflight_count: runtime.max_inflight_count,
            auth: None,
            external_auth: None,
            publish_auth: None,
            publish_ack: None,
            subscribe_auth: None,
            dynamic_filters: true,
        },
    };
    if telemetry_sink.remote_ack_gate_enabled() {
        let ack_store = store.clone();
        let ack_sink = telemetry_sink.clone();
        let ack_identities = client_identities.clone();
        let require_connection_identity = runtime.require_cert_fingerprint_username;
        server.set_publish_ack_handler(move |client_id, topic, payload| {
            let ack_store = ack_store.clone();
            let ack_sink = ack_sink.clone();
            let ack_identities = ack_identities.clone();
            async move {
                match durably_accept_remote_publish(
                    &ack_store,
                    &ack_sink,
                    &ack_identities,
                    Some(&client_id),
                    &topic,
                    &payload,
                    require_connection_identity,
                )
                .await
                {
                    Ok(Some(count)) => {
                        info!(
                            %client_id,
                            %topic,
                            count,
                            "durably accepted MQTT telemetry before broker ack"
                        );
                        true
                    }
                    Ok(None) => true,
                    Err(error) => {
                        warn!(
                            %client_id,
                            %topic,
                            %error,
                            "rejecting MQTT publish before broker ack"
                        );
                        false
                    }
                }
            }
        });
    }
    if runtime.require_cert_fingerprint_username {
        let bind_peer_certificate = runtime.tls.is_some();
        let auth_identities = client_identities.clone();
        server.set_auth_handler_with_peer(move |client_id, username, _password, peer_fingerprint| {
            let store = store.clone();
            let auth_identities = auth_identities.clone();
            async move {
                if client_id.trim().is_empty() {
                    warn!("rejecting MQTT auth with empty client id");
                    return false;
                }
                let username_fingerprint = username.trim().to_ascii_lowercase();
                let fingerprint = if bind_peer_certificate {
                    let Some(peer_fingerprint) = peer_fingerprint else {
                        warn!(client_id, "rejecting MQTT auth without TLS peer certificate fingerprint");
                        return false;
                    };
                    let peer_fingerprint = peer_fingerprint.to_ascii_lowercase();
                    if !username_fingerprint.is_empty()
                        && username_fingerprint != peer_fingerprint
                    {
                        warn!(client_id, "rejecting MQTT auth with mismatched username and peer certificate fingerprint");
                        return false;
                    }
                    peer_fingerprint
                } else {
                    username_fingerprint
                };
                match authenticate_device_certificate_fingerprint(&store, &fingerprint).await {
                    Ok(device) => match auth_identities.write() {
                        Ok(mut identities) => {
                            identities.insert(
                                client_id,
                                MqttClientIdentity {
                                    fingerprint_sha256: fingerprint,
                                    device,
                                },
                            );
                            true
                        }
                        Err(_) => false,
                    },
                    Err(_) => false,
                }
            }
        });
        let subscribe_identities = client_identities.clone();
        server.set_subscribe_auth_handler(move |client_id, filter| {
            let Ok(Some(identity)) = lookup_client_identity(&subscribe_identities, &client_id)
            else {
                return false;
            };
            excalibur_mqtt_ingest::authorize_subscribe(&filter, identity.device).is_ok()
        });
        let publish_identities = client_identities.clone();
        server.set_publish_auth_handler(move |client_id, topic| {
            let Ok(Some(identity)) = lookup_client_identity(&publish_identities, &client_id) else {
                return false;
            };
            excalibur_mqtt_ingest::authorize_publish(&topic, identity.device).is_ok()
        });
    }
    v4.insert("excalibur".to_owned(), server);

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
    fn fingerprint_identity_requires_tls_unless_dev_override_is_enabled() {
        assert!(validate_mqtt_identity_binding(true, true, false).is_ok());
        assert!(validate_mqtt_identity_binding(false, true, true).is_ok());

        let error = validate_mqtt_identity_binding(false, true, false).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("MQTT_REQUIRE_CERT_FINGERPRINT_USERNAME requires MQTT TLS")
        );
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
            tls: Some(MqttTlsConfig {
                ca_cert_path: "/etc/excalibur/ca.pem".to_owned(),
                server_cert_path: "/etc/excalibur/server.pem".to_owned(),
                server_key_path: "/etc/excalibur/server.key".to_owned(),
            }),
            require_cert_fingerprint_username: true,
            telemetry_buffer: TelemetryBufferConfig::Direct,
            command_bridge: CommandBridgeConfig::Disabled,
            storage: StorageConfig::Memory,
        };

        let config = rumqttd_config(
            &runtime,
            Store::memory(),
            TelemetrySink::Direct,
            Arc::new(RwLock::new(HashMap::new())),
        );
        let server = config.v4.as_ref().unwrap().get("excalibur").unwrap();

        assert_eq!(server.listen, runtime.listen);
        assert_eq!(server.connections.max_payload_size, 4096);
        assert!(server.connections.external_auth.is_some());
        assert!(server.connections.publish_auth.is_some());
        assert!(server.connections.publish_ack.is_none());
        assert!(server.connections.subscribe_auth.is_some());
        assert_eq!(
            server.tls.as_ref().unwrap().validate_paths(),
            false,
            "test paths are intentionally absent"
        );
        assert_eq!(
            config.router.initialized_filters.unwrap(),
            vec![
                "v1/p/+/d/+/telemetry/+".to_owned(),
                "v1/p/+/d/+/shadow".to_owned(),
                "v1/p/+/d/+/commands/status".to_owned(),
            ]
        );
    }

    #[test]
    fn rumqttd_config_attaches_publish_ack_gate_for_nats_telemetry() {
        let runtime = MqttRuntimeConfig {
            listen: "127.0.0.1:18832".parse().unwrap(),
            max_connections: 10,
            max_payload_size: 4096,
            max_inflight_count: 8,
            connection_timeout_ms: 1000,
            router_max_outgoing_packet_count: 32,
            router_max_segment_size: 1024,
            router_max_segment_count: 2,
            tls: None,
            require_cert_fingerprint_username: false,
            telemetry_buffer: TelemetryBufferConfig::Nats {
                url: "nats://127.0.0.1:4222".to_owned(),
                subject: "excalibur.telemetry.ingest".to_owned(),
                stream: "EXCALIBUR_TELEMETRY".to_owned(),
            },
            command_bridge: CommandBridgeConfig::Disabled,
            storage: StorageConfig::Memory,
        };
        let sink = TelemetrySink::Nats {
            client: NatsClient::new("nats://127.0.0.1:4222", "ack-gate-config-test").unwrap(),
            subject: "excalibur.telemetry.ingest".to_owned(),
            stream: "EXCALIBUR_TELEMETRY".to_owned(),
        };

        let config = rumqttd_config(
            &runtime,
            Store::memory(),
            sink,
            Arc::new(RwLock::new(HashMap::new())),
        );
        let server = config.v4.as_ref().unwrap().get("excalibur").unwrap();

        assert!(server.connections.publish_ack.is_some());
    }

    #[tokio::test]
    async fn rumqttd_publish_ack_gate_rejects_invalid_excalibur_topic() {
        let runtime = MqttRuntimeConfig {
            listen: "127.0.0.1:18833".parse().unwrap(),
            max_connections: 10,
            max_payload_size: 4096,
            max_inflight_count: 8,
            connection_timeout_ms: 1000,
            router_max_outgoing_packet_count: 32,
            router_max_segment_size: 1024,
            router_max_segment_count: 2,
            tls: None,
            require_cert_fingerprint_username: false,
            telemetry_buffer: TelemetryBufferConfig::Nats {
                url: "nats://127.0.0.1:4222".to_owned(),
                subject: "excalibur.telemetry.ingest".to_owned(),
                stream: "EXCALIBUR_TELEMETRY".to_owned(),
            },
            command_bridge: CommandBridgeConfig::Disabled,
            storage: StorageConfig::Memory,
        };
        let sink = TelemetrySink::Nats {
            client: NatsClient::new("nats://127.0.0.1:9", "ack-gate-invalid-topic-test").unwrap(),
            subject: "excalibur.telemetry.ingest".to_owned(),
            stream: "EXCALIBUR_TELEMETRY".to_owned(),
        };

        let config = rumqttd_config(
            &runtime,
            Store::memory(),
            sink,
            Arc::new(RwLock::new(HashMap::new())),
        );
        let server = config.v4.as_ref().unwrap().get("excalibur").unwrap();
        let ack = server.connections.publish_ack.as_ref().unwrap().clone();

        assert!(
            !ack(
                "client-1".to_owned(),
                "v1/p/not-a-uuid/d/not-a-uuid/telemetry/temperature".to_owned(),
                br#"[{"sequence":1}]"#.to_vec(),
            )
            .await
        );
    }

    #[tokio::test]
    async fn forward_source_identity_blocks_cross_device_publish() {
        use excalibur_domain::{Device, DeviceCertificate, Org, Project, User};

        let store = Store::memory();
        let user = store
            .create_user(User::new("mqtt-source@example.com", "MQTT Source", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("MQTT Source Org", "mqtt-source"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let source_device = store
            .create_device(Device::new(project.id, "source-device", json!({})))
            .await
            .unwrap();
        let other_device = store
            .create_device(Device::new(project.id, "other-device", json!({})))
            .await
            .unwrap();
        let fingerprint = "a".repeat(64);
        store
            .create_device_certificate(DeviceCertificate::new(
                project.id,
                source_device.id,
                fingerprint.clone(),
                Utc::now() + chrono::Duration::days(30),
            ))
            .await
            .unwrap();
        let identities = Arc::new(RwLock::new(HashMap::from([(
            source_device.id.to_string(),
            MqttClientIdentity {
                fingerprint_sha256: fingerprint,
                device: AuthenticatedDevice {
                    project_id: project.id,
                    device_id: source_device.id,
                    status: DeviceStatus::Provisioned,
                },
            },
        )])));
        let topic =
            excalibur_device_protocol::telemetry_topic(project.id, other_device.id, "temperature");
        let authenticated = authenticated_device_for_forward(
            &store,
            &identities,
            Some(&format!("excalibur.{}", source_device.id)),
            &topic,
            true,
        )
        .await
        .unwrap();

        let error = handle_ingest_publish(
            &store,
            &TelemetrySink::Direct,
            &topic,
            json!([{ "sequence": 1, "timestamp": Utc::now().to_rfc3339(), "value": 24.0 }]),
            authenticated,
        )
        .await
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not match authenticated device")
        );
        assert!(
            authenticated_device_for_forward(&store, &identities, None, &topic, true)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn local_ingest_skips_telemetry_when_remote_ack_gate_is_enabled() {
        use excalibur_domain::{Device, Org, Project, User};

        let store = Store::memory();
        let user = store
            .create_user(User::new("mqtt-skip@example.com", "MQTT Skip", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("MQTT Skip Org", "mqtt-skip"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let topic =
            excalibur_device_protocol::telemetry_topic(project.id, device.id, "temperature");
        let sink = TelemetrySink::Nats {
            client: NatsClient::new("nats://127.0.0.1:9", "skip-duplicate-test").unwrap(),
            subject: "excalibur.telemetry.ingest".to_owned(),
            stream: "EXCALIBUR_TELEMETRY".to_owned(),
        };

        let outcome = handle_ingest_publish(
            &store,
            &sink,
            &topic,
            json!([{ "sequence": 1, "timestamp": Utc::now().to_rfc3339(), "value": 24.0 }]),
            AuthenticatedDevice {
                project_id: project.id,
                device_id: device.id,
                status: DeviceStatus::Provisioned,
            },
        )
        .await
        .unwrap();

        assert_eq!(outcome, IngestPublishOutcome::DurablyAcceptedUpstream);
        assert!(
            store
                .query_telemetry(project.id, Some(device.id), Some("temperature"), 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn remote_ack_gate_rejects_invalid_telemetry_payload_before_queueing() {
        use excalibur_domain::{Device, Org, Project, User};

        let store = Store::memory();
        let user = store
            .create_user(User::new(
                "mqtt-invalid@example.com",
                "MQTT Invalid",
                "hash",
            ))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("MQTT Invalid Org", "mqtt-invalid"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let topic =
            excalibur_device_protocol::telemetry_topic(project.id, device.id, "temperature");
        let sink = TelemetrySink::Nats {
            client: NatsClient::new("nats://127.0.0.1:9", "invalid-payload-test").unwrap(),
            subject: "excalibur.telemetry.ingest".to_owned(),
            stream: "EXCALIBUR_TELEMETRY".to_owned(),
        };

        let error = durably_accept_remote_publish(
            &store,
            &sink,
            &Arc::new(RwLock::new(HashMap::new())),
            None,
            &topic,
            b"{",
            false,
        )
        .await
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("telemetry payload is not valid JSON")
        );
        assert!(
            store
                .query_telemetry(project.id, Some(device.id), Some("temperature"), 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn remote_ack_gate_rejects_invalid_excalibur_topic_before_queueing() {
        let store = Store::memory();
        let sink = TelemetrySink::Nats {
            client: NatsClient::new("nats://127.0.0.1:9", "invalid-topic-test").unwrap(),
            subject: "excalibur.telemetry.ingest".to_owned(),
            stream: "EXCALIBUR_TELEMETRY".to_owned(),
        };

        let error = durably_accept_remote_publish(
            &store,
            &sink,
            &Arc::new(RwLock::new(HashMap::new())),
            None,
            "v1/p/not-a-uuid/d/not-a-uuid/telemetry/temperature",
            br#"[{"sequence":1}]"#,
            false,
        )
        .await
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("invalid Excalibur publish topic")
        );
    }

    #[tokio::test]
    async fn remote_ack_gate_ignores_non_excalibur_publish_topic() {
        let store = Store::memory();
        let sink = TelemetrySink::Nats {
            client: NatsClient::new("nats://127.0.0.1:9", "non-excalibur-topic-test").unwrap(),
            subject: "excalibur.telemetry.ingest".to_owned(),
            stream: "EXCALIBUR_TELEMETRY".to_owned(),
        };

        let accepted = durably_accept_remote_publish(
            &store,
            &sink,
            &Arc::new(RwLock::new(HashMap::new())),
            None,
            "external/topic",
            br#"[{"sequence":1}]"#,
            false,
        )
        .await
        .unwrap();

        assert_eq!(accepted, None);
    }

    #[tokio::test]
    async fn remote_ack_gate_rejects_when_jetstream_publish_fails() {
        use excalibur_domain::{Device, Org, Project, User};

        let store = Store::memory();
        let user = store
            .create_user(User::new(
                "mqtt-nats-fail@example.com",
                "MQTT NATS Fail",
                "hash",
            ))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("MQTT NATS Fail Org", "mqtt-nats-fail"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let topic =
            excalibur_device_protocol::telemetry_topic(project.id, device.id, "temperature");
        let sink = TelemetrySink::Nats {
            client: NatsClient::new("nats://127.0.0.1:9", "nats-failure-test").unwrap(),
            subject: "excalibur.telemetry.ingest".to_owned(),
            stream: "EXCALIBUR_TELEMETRY".to_owned(),
        };
        let payload = serde_json::to_vec(
            &json!([{ "sequence": 1, "timestamp": Utc::now().to_rfc3339(), "value": 24.0 }]),
        )
        .unwrap();

        let error = durably_accept_remote_publish(
            &store,
            &sink,
            &Arc::new(RwLock::new(HashMap::new())),
            None,
            &topic,
            &payload,
            false,
        )
        .await
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("failed to publish telemetry envelope")
        );
        assert!(
            store
                .query_telemetry(project.id, Some(device.id), Some("temperature"), 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rumqttd_auth_binds_username_fingerprint_to_tls_peer_certificate() {
        use excalibur_domain::{Device, DeviceCertificate, Org, Project, User};

        let store = Store::memory();
        let user = store
            .create_user(User::new("mqtt-auth@example.com", "MQTT Auth", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("MQTT Auth Org", "mqtt-auth"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let fingerprint = "a".repeat(64);
        store
            .create_device_certificate(DeviceCertificate::new(
                project.id,
                device.id,
                fingerprint.clone(),
                Utc::now() + chrono::Duration::days(30),
            ))
            .await
            .unwrap();

        let runtime = MqttRuntimeConfig {
            listen: "127.0.0.1:18831".parse().unwrap(),
            max_connections: 10,
            max_payload_size: 4096,
            max_inflight_count: 8,
            connection_timeout_ms: 1000,
            router_max_outgoing_packet_count: 32,
            router_max_segment_size: 1024,
            router_max_segment_count: 2,
            tls: Some(MqttTlsConfig {
                ca_cert_path: "/etc/excalibur/ca.pem".to_owned(),
                server_cert_path: "/etc/excalibur/server.pem".to_owned(),
                server_key_path: "/etc/excalibur/server.key".to_owned(),
            }),
            require_cert_fingerprint_username: true,
            telemetry_buffer: TelemetryBufferConfig::Direct,
            command_bridge: CommandBridgeConfig::Disabled,
            storage: StorageConfig::Memory,
        };
        let identities = Arc::new(RwLock::new(HashMap::new()));
        let config = rumqttd_config(&runtime, store, TelemetrySink::Direct, identities.clone());
        let server = config.v4.as_ref().unwrap().get("excalibur").unwrap();
        let auth = server.connections.external_auth.as_ref().unwrap().clone();

        assert!(
            auth(
                device.id.to_string(),
                fingerprint.clone(),
                String::new(),
                Some(fingerprint.clone()),
            )
            .await
        );
        assert!(
            lookup_client_identity(&identities, &device.id.to_string())
                .unwrap()
                .is_some()
        );
        assert!(
            !auth(
                String::new(),
                fingerprint.clone(),
                String::new(),
                Some(fingerprint.clone()),
            )
            .await
        );
        assert!(lookup_client_identity(&identities, "").unwrap().is_none());
        assert!(
            !auth(
                "mismatch".to_owned(),
                "b".repeat(64),
                String::new(),
                Some(fingerprint.clone()),
            )
            .await
        );
        assert!(!auth("missing-peer".to_owned(), fingerprint, String::new(), None,).await);
    }

    #[test]
    fn command_bridge_publishes_device_command_payload_to_envelope_topic() {
        let project_id = Id::now_v7();
        let device_id = Id::now_v7();
        let action_id = Id::now_v7();
        let envelope = DeviceCommandEnvelope {
            project_id,
            device_id,
            topic: commands_topic(project_id, device_id),
            command: excalibur_device_protocol::command_for_action(
                action_id,
                "diagnostics.collect",
                json!({ "paths": ["/var/log"] }),
            ),
        };
        let payload = serde_json::to_vec(&envelope).unwrap();

        let (topic, command_payload) = mqtt_command_publish_from_nats_payload(&payload).unwrap();

        assert_eq!(topic, commands_topic(project_id, device_id));
        assert_eq!(
            serde_json::from_slice::<Value>(&command_payload).unwrap(),
            json!({
                "action_id": action_id,
                "name": "diagnostics.collect",
                "payload": { "paths": ["/var/log"] }
            })
        );
    }

    #[test]
    fn command_bridge_rejects_mismatched_envelope_topic() {
        let project_id = Id::now_v7();
        let device_id = Id::now_v7();
        let envelope = DeviceCommandEnvelope {
            project_id,
            device_id,
            topic: commands_topic(project_id, Id::now_v7()),
            command: excalibur_device_protocol::command_for_action(
                Id::now_v7(),
                "diagnostics.collect",
                json!({}),
            ),
        };
        let payload = serde_json::to_vec(&envelope).unwrap();

        assert!(mqtt_command_publish_from_nats_payload(&payload).is_err());
    }
}
