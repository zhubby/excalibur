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
    DeviceCommand, DeviceCommandEnvelope, OtaInstallPayload, PublishTopic, TelemetryIngestEnvelope,
    commands_topic, parse_publish_topic,
};
use excalibur_domain::{ActionState, DeviceStatus, FirmwareArtifact, Id};
use excalibur_mqtt_ingest::{
    AuthenticatedDevice, authenticate_device_certificate_fingerprint, ingest_publish,
    telemetry_envelope_from_publish,
};
use excalibur_nats_lite::{NatsClient, NatsMessage};
use excalibur_object_storage::{ObjectStorageConfig, presigned_object_key_url};
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
        dead_letter_subject: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommandBridgeConfig {
    Disabled,
    Nats(Box<NatsCommandBridgeConfig>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NatsCommandBridgeConfig {
    url: String,
    subject: String,
    stream: String,
    delivery_subject: String,
    durable_name: String,
    queue_group: String,
    dead_letter_subject: String,
    download_url_ttl_seconds: i64,
    object_storage: ObjectStorageConfig,
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
        dead_letter_subject: String,
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
                dead_letter_subject,
            } => Ok(Self::Nats {
                client: NatsClient::new(url, "excalibur-mqtt-ingest")?,
                subject: subject.clone(),
                stream: stream.clone(),
                dead_letter_subject: dead_letter_subject.clone(),
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
    let command_bridge_task =
        start_command_bridge_task(link_tx, store.clone(), runtime.command_bridge.clone());
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
        ..
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
        dead_letter_subject,
    } = telemetry_sink
    else {
        return Ok(());
    };
    let payload = json!({
        "name": stream,
        "subjects": [subject, dead_letter_subject],
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
    verify_jetstream_stream_subjects(client, stream, &[subject, dead_letter_subject]).await?;
    info!(
        stream,
        subject, dead_letter_subject, "JetStream telemetry stream ready"
    );
    Ok(())
}

fn start_command_bridge_task(
    link_tx: rumqttd::local::LinkTx,
    store: Store,
    command_bridge: CommandBridgeConfig,
) -> Option<tokio::task::JoinHandle<anyhow::Result<()>>> {
    let CommandBridgeConfig::Nats(config) = command_bridge else {
        info!("MQTT command bridge disabled");
        return None;
    };
    Some(tokio::spawn(async move {
        run_command_bridge_loop(link_tx, store, *config).await
    }))
}

async fn run_command_bridge_loop(
    mut link_tx: rumqttd::local::LinkTx,
    store: Store,
    config: NatsCommandBridgeConfig,
) -> anyhow::Result<()> {
    loop {
        if let Err(error) = run_command_bridge_subscription(&mut link_tx, &store, &config).await {
            warn!(
                %error,
                subject = %config.subject,
                delivery_subject = %config.delivery_subject,
                durable = %config.durable_name,
                "MQTT command bridge disconnected; retrying"
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}

async fn run_command_bridge_subscription(
    link_tx: &mut rumqttd::local::LinkTx,
    store: &Store,
    config: &NatsCommandBridgeConfig,
) -> anyhow::Result<()> {
    let client = NatsClient::new(&config.url, "excalibur-mqtt-command-bridge")?;
    ensure_command_bridge_stream(&client, config).await?;
    ensure_command_bridge_consumer(&client, config).await?;
    let mut subscription = client
        .subscribe(&config.delivery_subject, Some(&config.queue_group))
        .await
        .with_context(|| {
            format!(
                "failed to subscribe command bridge to {}",
                config.delivery_subject
            )
        })?;
    info!(
        subject = %config.subject,
        stream = %config.stream,
        delivery_subject = %config.delivery_subject,
        durable = %config.durable_name,
        queue = %config.queue_group,
        "MQTT command bridge ready"
    );

    loop {
        let message = subscription.next_message().await?;
        handle_command_bridge_message(link_tx, store, &client, config, message).await?;
    }
}

async fn ensure_command_bridge_stream(
    client: &NatsClient,
    config: &NatsCommandBridgeConfig,
) -> anyhow::Result<()> {
    let payload = json!({
        "name": config.stream,
        "subjects": [config.subject, config.dead_letter_subject],
        "retention": "limits",
        "storage": "file",
        "discard": "old",
        "max_msgs": -1,
        "max_bytes": -1,
        "max_age": 0,
        "max_msg_size": -1
    });
    let response = client
        .request(
            &format!("$JS.API.STREAM.CREATE.{}", config.stream),
            payload.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .await
        .with_context(|| {
            format!(
                "failed to ensure JetStream command stream {}",
                config.stream
            )
        })?;
    let response_json = serde_json::from_slice::<Value>(&response.payload)
        .context("JetStream command stream create response was not JSON")?;
    if let Some(error) = response_json.get("error") {
        let description = error
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("unknown JetStream error");
        if !description.contains("already in use") && !description.contains("already exists") {
            bail!("JetStream command stream create failed: {description}");
        }
    }
    verify_jetstream_stream_subjects(
        client,
        &config.stream,
        &[&config.subject, &config.dead_letter_subject],
    )
    .await?;
    Ok(())
}

async fn ensure_command_bridge_consumer(
    client: &NatsClient,
    config: &NatsCommandBridgeConfig,
) -> anyhow::Result<()> {
    let payload = json!({
        "stream_name": config.stream,
        "config": {
            "durable_name": config.durable_name,
            "deliver_subject": config.delivery_subject,
            "deliver_group": config.queue_group,
            "filter_subject": config.subject,
            "deliver_policy": "all",
            "ack_policy": "explicit",
            "max_ack_pending": 1024,
        }
    });
    let response = client
        .request(
            &format!(
                "$JS.API.CONSUMER.DURABLE.CREATE.{}.{}",
                config.stream, config.durable_name
            ),
            payload.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .await
        .with_context(|| {
            format!(
                "failed to ensure JetStream command consumer {} on {}",
                config.durable_name, config.stream
            )
        })?;
    let response_json = serde_json::from_slice::<Value>(&response.payload)
        .context("JetStream command consumer create response was not JSON")?;
    if let Some(error) = response_json.get("error") {
        let description = error
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("unknown JetStream error");
        let already_exists =
            description.contains("already in use") || description.contains("already exists");
        if !already_exists {
            bail!("JetStream command consumer create failed: {description}");
        }
    }
    verify_command_bridge_consumer_config(client, config).await?;
    Ok(())
}

async fn verify_command_bridge_consumer_config(
    client: &NatsClient,
    config: &NatsCommandBridgeConfig,
) -> anyhow::Result<()> {
    let response = client
        .request(
            &format!(
                "$JS.API.CONSUMER.INFO.{}.{}",
                config.stream, config.durable_name
            ),
            b"{}",
            Duration::from_secs(5),
        )
        .await
        .with_context(|| {
            format!(
                "failed to read JetStream command consumer info for {} on {}",
                config.durable_name, config.stream
            )
        })?;
    let response_json = serde_json::from_slice::<Value>(&response.payload)
        .context("JetStream command consumer info response was not JSON")?;
    fail_on_jetstream_error(&response_json, "command consumer info")?;
    let consumer_config = response_json
        .get("config")
        .ok_or_else(|| anyhow::anyhow!("JetStream command consumer info missing config"))?;
    validate_command_consumer_config_fields(consumer_config, config)
}

fn validate_command_consumer_config_fields(
    consumer_config: &Value,
    config: &NatsCommandBridgeConfig,
) -> anyhow::Result<()> {
    let expected = [
        ("durable_name", config.durable_name.as_str()),
        ("deliver_subject", config.delivery_subject.as_str()),
        ("deliver_group", config.queue_group.as_str()),
        ("filter_subject", config.subject.as_str()),
    ];
    for (field, expected_value) in expected {
        let actual = consumer_config
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("JetStream command consumer missing {field}"))?;
        if actual != expected_value {
            bail!(
                "JetStream command consumer {field} mismatch: expected {expected_value}, got {actual}"
            );
        }
    }
    let ack_policy = consumer_config.get("ack_policy");
    let explicit_ack = ack_policy
        .and_then(Value::as_str)
        .is_some_and(|value| value == "explicit")
        || ack_policy
            .and_then(Value::as_i64)
            .is_some_and(|value| value == 2);
    if !explicit_ack {
        bail!("JetStream command consumer ack_policy mismatch: expected explicit");
    }
    Ok(())
}

async fn verify_jetstream_stream_subjects(
    client: &NatsClient,
    stream: &str,
    expected_subjects: &[&str],
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
    fail_on_jetstream_error(&response_json, "stream info")?;
    let subjects = response_json
        .pointer("/config/subjects")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("JetStream stream info missing config.subjects"))?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    for expected in expected_subjects {
        if !subjects.iter().any(|subject| subject == expected) {
            bail!("JetStream stream {stream} is missing expected subject {expected}");
        }
    }
    Ok(())
}

fn fail_on_jetstream_error(value: &Value, context: &str) -> anyhow::Result<()> {
    if let Some(error) = value.get("error") {
        let description = error
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("unknown JetStream error");
        bail!("JetStream {context} failed: {description}");
    }
    Ok(())
}

async fn handle_command_bridge_message(
    link_tx: &mut rumqttd::local::LinkTx,
    store: &Store,
    client: &NatsClient,
    config: &NatsCommandBridgeConfig,
    message: NatsMessage,
) -> anyhow::Result<()> {
    let ack_subject = message
        .reply
        .as_deref()
        .context("JetStream command delivery is missing ack subject")?;
    let envelope = match decode_command_bridge_envelope(&message.payload) {
        Ok(envelope) => envelope,
        Err(error) => {
            warn!(%error, "dead-lettering invalid command bridge envelope");
            publish_command_dead_letter(client, config, &message.payload).await?;
            client
                .ack(ack_subject)
                .await
                .context("failed to ack invalid command bridge message")?;
            return Ok(());
        }
    };
    let current_state = command_bridge_envelope_state(store, &envelope).await?;
    if current_state != ActionState::Running {
        client
            .ack(ack_subject)
            .await
            .context("failed to ack stale command bridge message")?;
        info!(
            action_id = %envelope.command.action_id,
            device_id = %envelope.device_id,
            state = ?current_state,
            "acked stale action command without MQTT publish"
        );
        return Ok(());
    }

    match mqtt_command_publish_from_envelope(store, config, envelope).await {
        Ok((topic, payload)) => {
            link_tx
                .publish(topic.clone(), payload)
                .with_context(|| format!("failed to publish command to MQTT topic {topic}"))?;
            client
                .ack(ack_subject)
                .await
                .context("failed to ack command bridge message")?;
            info!(%topic, "published and acked action command to MQTT broker");
        }
        Err(CommandPayloadError::Permanent(error)) => {
            warn!(%error, "dead-lettering invalid command bridge payload");
            publish_command_dead_letter(client, config, &message.payload).await?;
            client
                .ack(ack_subject)
                .await
                .context("failed to ack invalid command bridge payload")?;
        }
        Err(CommandPayloadError::Transient(error)) => return Err(error),
    }
    Ok(())
}

async fn command_bridge_envelope_state(
    store: &Store,
    envelope: &DeviceCommandEnvelope,
) -> anyhow::Result<ActionState> {
    store
        .get_action_target_state(
            envelope.project_id,
            envelope.command.action_id,
            envelope.device_id,
        )
        .await
        .context("failed to read action target state before MQTT command publish")
}

async fn publish_command_dead_letter(
    client: &NatsClient,
    config: &NatsCommandBridgeConfig,
    payload: &[u8],
) -> anyhow::Result<()> {
    let ack = client
        .publish_jetstream(&config.dead_letter_subject, payload, Duration::from_secs(5))
        .await
        .with_context(|| {
            format!(
                "failed to publish command dead-letter to {}",
                config.dead_letter_subject
            )
        })?;
    if ack.stream != config.stream {
        bail!(
            "JetStream command dead-letter ack stream mismatch: expected {}, got {}",
            config.stream,
            ack.stream
        );
    }
    Ok(())
}

fn decode_command_bridge_envelope(payload: &[u8]) -> anyhow::Result<DeviceCommandEnvelope> {
    let envelope = serde_json::from_slice::<DeviceCommandEnvelope>(payload)
        .context("command bridge payload is not a DeviceCommandEnvelope")?;
    let expected_topic = commands_topic(envelope.project_id, envelope.device_id);
    if envelope.topic != expected_topic {
        bail!("command envelope topic does not match project/device identity");
    }
    Ok(envelope)
}

#[derive(Debug)]
enum CommandPayloadError {
    Permanent(anyhow::Error),
    Transient(anyhow::Error),
}

async fn mqtt_command_publish_from_envelope(
    store: &Store,
    config: &NatsCommandBridgeConfig,
    envelope: DeviceCommandEnvelope,
) -> Result<(String, Vec<u8>), CommandPayloadError> {
    let command =
        command_for_bridge_dispatch(store, config, envelope.command, envelope.project_id).await?;
    let payload = serde_json::to_vec(&command).map_err(|error| {
        CommandPayloadError::Permanent(
            anyhow::Error::from(error).context("failed to encode device command payload"),
        )
    })?;
    Ok((envelope.topic, payload))
}

async fn command_for_bridge_dispatch(
    store: &Store,
    config: &NatsCommandBridgeConfig,
    mut command: DeviceCommand,
    project_id: Id,
) -> Result<DeviceCommand, CommandPayloadError> {
    if command.name != "ota.install" {
        return Ok(command);
    }
    command.payload =
        ota_install_payload_for_bridge_dispatch(store, config, project_id, &command.payload)
            .await?;
    Ok(command)
}

async fn ota_install_payload_for_bridge_dispatch(
    store: &Store,
    config: &NatsCommandBridgeConfig,
    project_id: Id,
    payload: &Value,
) -> Result<Value, CommandPayloadError> {
    let firmware_id = payload
        .get("firmware_id")
        .and_then(Value::as_str)
        .and_then(|value| Id::parse_str(value).ok())
        .ok_or_else(|| {
            CommandPayloadError::Permanent(anyhow::anyhow!(
                "ota.install command payload is missing firmware_id"
            ))
        })?;
    let artifacts = store.list_firmware(project_id).await.map_err(|error| {
        CommandPayloadError::Transient(
            anyhow::Error::from(error).context("failed to load firmware for ota.install command"),
        )
    })?;
    let artifact = artifacts
        .into_iter()
        .find(|artifact| artifact.id == firmware_id && artifact.active)
        .ok_or_else(|| {
            CommandPayloadError::Permanent(anyhow::anyhow!(
                "firmware not found for ota.install command"
            ))
        })?;
    validate_ota_reference_for_bridge(project_id, payload, &artifact)?;
    let ttl = chrono::Duration::seconds(config.download_url_ttl_seconds.clamp(60, 3600));
    let signed_url =
        presigned_object_key_url(&config.object_storage, &artifact.object_key, "GET", ttl)
            .map_err(|error| {
                CommandPayloadError::Transient(
                    anyhow::Error::from(error)
                        .context("failed to sign firmware download URL for command bridge"),
                )
            })?
            .url;
    let payload = OtaInstallPayload {
        firmware_id: artifact.id,
        component: artifact.component,
        version: artifact.version,
        signed_url,
        sha256: artifact.sha256,
        signature: artifact.signature,
        size_bytes: artifact.size_bytes,
    };
    payload.validate().map_err(|error| {
        CommandPayloadError::Permanent(
            anyhow::Error::from(error).context("generated ota.install payload is invalid"),
        )
    })?;
    serde_json::to_value(payload).map_err(|error| {
        CommandPayloadError::Permanent(
            anyhow::Error::from(error).context("failed to encode ota.install payload"),
        )
    })
}

fn validate_ota_reference_for_bridge(
    project_id: Id,
    payload: &Value,
    artifact: &FirmwareArtifact,
) -> Result<(), CommandPayloadError> {
    let expected_prefix = format!("projects/{project_id}/firmware/");
    if !artifact.object_key.starts_with(&expected_prefix) {
        return Err(CommandPayloadError::Permanent(anyhow::anyhow!(
            "firmware object_key must stay under its project prefix"
        )));
    }
    if artifact.verified_at.is_none() {
        return Err(CommandPayloadError::Permanent(anyhow::anyhow!(
            "firmware must be finalized before ota.install command dispatch"
        )));
    }
    let expected_signature = serde_json::to_value(&artifact.signature).map_err(|error| {
        CommandPayloadError::Permanent(
            anyhow::Error::from(error).context("failed to encode firmware signature"),
        )
    })?;
    let matches = payload
        .get("component")
        .and_then(Value::as_str)
        .is_some_and(|value| value == artifact.component)
        && payload
            .get("version")
            .and_then(Value::as_str)
            .is_some_and(|value| value == artifact.version)
        && payload
            .get("sha256")
            .and_then(Value::as_str)
            .is_some_and(|value| value == artifact.sha256)
        && payload
            .get("size_bytes")
            .and_then(Value::as_i64)
            .is_some_and(|value| value == artifact.size_bytes)
        && payload.get("signature") == Some(&expected_signature);
    if !matches {
        return Err(CommandPayloadError::Permanent(anyhow::anyhow!(
            "ota.install command payload does not match firmware metadata"
        )));
    }
    Ok(())
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
    let dead_letter_subject = std::env::var("MQTT_TELEMETRY_DEAD_LETTER_SUBJECT")
        .unwrap_or_else(|_| "excalibur.telemetry.dead_letter".to_owned());
    match std::env::var("MQTT_TELEMETRY_BUFFER")
        .unwrap_or_else(|_| "auto".to_owned())
        .as_str()
    {
        "direct" => Ok(TelemetryBufferConfig::Direct),
        "nats" => Ok(TelemetryBufferConfig::Nats {
            url: std::env::var("NATS_URL").context("NATS_URL is required for nats buffer")?,
            subject,
            stream,
            dead_letter_subject,
        }),
        "auto" => match std::env::var("NATS_URL") {
            Ok(url) => Ok(TelemetryBufferConfig::Nats {
                url,
                subject,
                stream,
                dead_letter_subject,
            }),
            Err(_) => Ok(TelemetryBufferConfig::Direct),
        },
        value => bail!("unsupported MQTT_TELEMETRY_BUFFER={value}; expected auto, direct, or nats"),
    }
}

fn command_bridge_config_from_env() -> anyhow::Result<CommandBridgeConfig> {
    let subject = std::env::var("MQTT_COMMAND_NATS_SUBJECT")
        .unwrap_or_else(|_| "excalibur.commands.dispatch".to_owned());
    let stream = std::env::var("MQTT_COMMAND_NATS_STREAM")
        .unwrap_or_else(|_| "EXCALIBUR_COMMANDS".to_owned());
    let delivery_subject = std::env::var("MQTT_COMMAND_DELIVERY_SUBJECT")
        .unwrap_or_else(|_| "excalibur.commands.deliver".to_owned());
    let durable_name = std::env::var("MQTT_COMMAND_DURABLE")
        .unwrap_or_else(|_| "excalibur-mqtt-command-bridge".to_owned());
    let queue_group =
        std::env::var("MQTT_COMMAND_QUEUE_GROUP").unwrap_or_else(|_| durable_name.clone());
    let dead_letter_subject = std::env::var("MQTT_COMMAND_DEAD_LETTER_SUBJECT")
        .unwrap_or_else(|_| "excalibur.commands.dead_letter".to_owned());
    let download_url_ttl_seconds = parse_env("MQTT_COMMAND_DOWNLOAD_URL_TTL_SECONDS", "900")?;
    match std::env::var("MQTT_COMMAND_BRIDGE")
        .unwrap_or_else(|_| "auto".to_owned())
        .as_str()
    {
        "disabled" => Ok(CommandBridgeConfig::Disabled),
        "nats" => {
            let object_storage = ObjectStorageConfig::from_env()
                .context("S3_PUBLIC_ENDPOINT or S3_ENDPOINT is required for MQTT command bridge")?;
            Ok(CommandBridgeConfig::Nats(Box::new(
                NatsCommandBridgeConfig {
                    url: std::env::var("NATS_URL")
                        .context("NATS_URL is required for command bridge")?,
                    subject,
                    stream,
                    delivery_subject,
                    durable_name,
                    queue_group,
                    dead_letter_subject,
                    download_url_ttl_seconds,
                    object_storage,
                },
            )))
        }
        "auto" => match std::env::var("NATS_URL") {
            Ok(url) => {
                let object_storage = ObjectStorageConfig::from_env().context(
                    "S3_PUBLIC_ENDPOINT or S3_ENDPOINT is required for MQTT command bridge",
                )?;
                Ok(CommandBridgeConfig::Nats(Box::new(
                    NatsCommandBridgeConfig {
                        url,
                        subject,
                        stream,
                        delivery_subject,
                        durable_name,
                        queue_group,
                        dead_letter_subject,
                        download_url_ttl_seconds,
                        object_storage,
                    },
                )))
            }
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
                dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
            },
            command_bridge: CommandBridgeConfig::Disabled,
            storage: StorageConfig::Memory,
        };
        let sink = TelemetrySink::Nats {
            client: NatsClient::new("nats://127.0.0.1:4222", "ack-gate-config-test").unwrap(),
            subject: "excalibur.telemetry.ingest".to_owned(),
            stream: "EXCALIBUR_TELEMETRY".to_owned(),
            dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
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
                dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
            },
            command_bridge: CommandBridgeConfig::Disabled,
            storage: StorageConfig::Memory,
        };
        let sink = TelemetrySink::Nats {
            client: NatsClient::new("nats://127.0.0.1:9", "ack-gate-invalid-topic-test").unwrap(),
            subject: "excalibur.telemetry.ingest".to_owned(),
            stream: "EXCALIBUR_TELEMETRY".to_owned(),
            dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
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
            dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
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
            dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
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
            dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
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
            dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
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
            dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
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

    #[tokio::test]
    async fn command_bridge_publishes_device_command_payload_to_envelope_topic() {
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

        let decoded_envelope = decode_command_bridge_envelope(&payload).unwrap();
        let (topic, command_payload) = mqtt_command_publish_from_envelope(
            &Store::memory(),
            &test_command_bridge_config(),
            decoded_envelope,
        )
        .await
        .unwrap();

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

    #[tokio::test]
    async fn command_bridge_signs_ota_url_after_command_stream_delivery() {
        use excalibur_domain::{Action, FirmwareArtifact, Org, Project, User};

        let store = Store::memory();
        let user = store
            .create_user(User::new("bridge-ota@example.com", "Bridge OTA", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Bridge OTA Org", "bridge-ota"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = store
            .create_device(excalibur_domain::Device::new(
                project.id,
                "press-1",
                json!({}),
            ))
            .await
            .unwrap();
        let firmware = store
            .create_firmware(FirmwareArtifact::new(
                project.id,
                "main",
                "1.0.0",
                format!("projects/{}/firmware/main.bin", project.id),
                "a".repeat(64),
                "application/octet-stream",
                Some("ed25519:test".to_owned()),
                1024,
            ))
            .await
            .unwrap();
        let firmware = store
            .finalize_firmware(
                project.id,
                firmware.id,
                &"a".repeat(64),
                1024,
                Some("ed25519:test"),
                Utc::now(),
            )
            .await
            .unwrap();
        let action = store
            .create_action(Action::new(
                project.id,
                vec![device.id],
                "ota.install",
                json!({
                    "firmware_id": firmware.id,
                    "component": "main",
                    "version": "1.0.0",
                    "sha256": "a".repeat(64),
                    "signature": "ed25519:test",
                    "size_bytes": 1024
                }),
                Some(user.id),
            ))
            .await
            .unwrap();
        store.claim_queued_action_targets(1).await.unwrap();
        let envelope = DeviceCommandEnvelope {
            project_id: project.id,
            device_id: device.id,
            topic: commands_topic(project.id, device.id),
            command: excalibur_device_protocol::command_for_action(
                action.id,
                "ota.install",
                action.payload.clone(),
            ),
        };
        let payload = serde_json::to_vec(&envelope).unwrap();

        let decoded_envelope = decode_command_bridge_envelope(&payload).unwrap();
        let (topic, command_payload) = mqtt_command_publish_from_envelope(
            &store,
            &test_command_bridge_config(),
            decoded_envelope,
        )
        .await
        .unwrap();
        let command = serde_json::from_slice::<DeviceCommand>(&command_payload).unwrap();

        assert_eq!(topic, commands_topic(project.id, device.id));
        assert!(envelope.command.payload.get("signed_url").is_none());
        assert_eq!(command.name, "ota.install");
        assert!(
            command.payload["signed_url"]
                .as_str()
                .unwrap()
                .contains("X-Amz-Signature=")
        );
        assert!(
            command.payload["signed_url"]
                .as_str()
                .unwrap()
                .contains(&format!("projects/{}/firmware/main.bin", project.id))
        );
    }

    #[tokio::test]
    async fn command_bridge_rejects_cancelled_stale_command_before_publish() {
        use excalibur_domain::{Action, ActionTargetTransition, Org, Project, User};

        let store = Store::memory();
        let user = store
            .create_user(User::new(
                "bridge-stale@example.com",
                "Bridge Stale",
                "hash",
            ))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Bridge Stale Org", "bridge-stale"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = store
            .create_device(excalibur_domain::Device::new(
                project.id,
                "press-1",
                json!({}),
            ))
            .await
            .unwrap();
        let action = store
            .create_action(Action::new(
                project.id,
                vec![device.id],
                "diagnostics.collect",
                json!({ "paths": ["/var/log"] }),
                Some(user.id),
            ))
            .await
            .unwrap();
        store.claim_queued_action_targets(1).await.unwrap();
        let envelope = DeviceCommandEnvelope {
            project_id: project.id,
            device_id: device.id,
            topic: commands_topic(project.id, device.id),
            command: excalibur_device_protocol::command_for_action(
                action.id,
                "diagnostics.collect",
                action.payload.clone(),
            ),
        };
        assert_eq!(
            command_bridge_envelope_state(&store, &envelope)
                .await
                .unwrap(),
            ActionState::Running
        );

        store
            .transition_action_targets(ActionTargetTransition {
                project_id: project.id,
                action_id: action.id,
                device_ids: Some(vec![device.id]),
                allowed_source_states: vec![ActionState::Running],
                next_state: ActionState::Cancelled,
                progress: None,
                errors: Some(vec!["operator cancelled".to_owned()]),
                ts: Utc::now(),
            })
            .await
            .unwrap();

        assert_eq!(
            command_bridge_envelope_state(&store, &envelope)
                .await
                .unwrap(),
            ActionState::Cancelled
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

        assert!(decode_command_bridge_envelope(&payload).is_err());
    }

    #[test]
    fn command_bridge_consumer_validation_requires_explicit_ack() {
        let config = test_command_bridge_config();
        let consumer_config = json!({
            "durable_name": config.durable_name,
            "deliver_subject": config.delivery_subject,
            "deliver_group": config.queue_group,
            "filter_subject": config.subject,
            "ack_policy": "none",
        });

        let error = validate_command_consumer_config_fields(&consumer_config, &config).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("ack_policy mismatch: expected explicit")
        );
    }

    #[test]
    fn command_bridge_consumer_validation_accepts_numeric_explicit_ack() {
        let config = test_command_bridge_config();
        let consumer_config = json!({
            "durable_name": config.durable_name,
            "deliver_subject": config.delivery_subject,
            "deliver_group": config.queue_group,
            "filter_subject": config.subject,
            "ack_policy": 2,
        });

        validate_command_consumer_config_fields(&consumer_config, &config).unwrap();
    }

    fn test_command_bridge_config() -> NatsCommandBridgeConfig {
        NatsCommandBridgeConfig {
            url: "nats://127.0.0.1:4222".to_owned(),
            subject: "excalibur.commands.dispatch".to_owned(),
            stream: "EXCALIBUR_COMMANDS".to_owned(),
            delivery_subject: "excalibur.commands.deliver".to_owned(),
            durable_name: "excalibur-mqtt-command-bridge".to_owned(),
            queue_group: "excalibur-mqtt-command-bridge".to_owned(),
            dead_letter_subject: "excalibur.commands.dead_letter".to_owned(),
            download_url_ttl_seconds: 900,
            object_storage: ObjectStorageConfig::development(),
        }
    }
}
