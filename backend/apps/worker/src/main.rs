use std::{
    str::FromStr,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use excalibur_device_protocol::{
    DeviceCommandEnvelope, TelemetryIngestEnvelope, command_for_action, commands_topic,
};
use excalibur_domain::{
    ActionDispatchTarget, ActionState, ActionTargetTransition, AlertEvent, AlertKind, AlertRule,
    Device, Id, NewAlertEvent,
};
use excalibur_mqtt_ingest::write_telemetry_envelope;
use excalibur_nats_lite::NatsClient;
use excalibur_storage::{PgStore, Store};
use serde_json::Value;
use tracing::{debug, info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone, PartialEq, Eq)]
enum StorageConfig {
    Memory,
    Sql { database_url: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkerConfig {
    storage: StorageConfig,
    nats_url: Option<String>,
    telemetry_subject: String,
    telemetry_stream: String,
    telemetry_delivery_subject: String,
    telemetry_dead_letter_subject: String,
    telemetry_queue_group: String,
    telemetry_batch_size: usize,
    telemetry_batch_window_ms: u64,
    action_command_subject: String,
    action_claim_limit: usize,
    action_dispatch_interval_ms: u64,
    action_timeout_seconds: u64,
    action_timeout_scan_limit: usize,
    alert_scan_interval_ms: u64,
    alert_default_offline_after_seconds: i64,
    alert_default_window_seconds: i64,
    alert_notification_subject: String,
}

#[derive(Debug, Clone)]
struct PendingTelemetryMessage {
    envelope: TelemetryIngestEnvelope,
    ack_subject: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "excalibur_worker=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = worker_config_from_env()?;
    info!("starting excalibur worker");
    let store = build_store(config.storage.clone()).await?;
    store.health_check().await?;

    let Some(nats_url) = config.nats_url.clone() else {
        info!("NATS_URL not set; worker heartbeat mode only");
        let alert_store = store.clone();
        let alert_config = config.clone();
        tokio::spawn(async move {
            run_alert_evaluation_loop(alert_store, None, alert_config).await;
        });
        return heartbeat_loop().await;
    };

    let client = NatsClient::new(nats_url, "excalibur-worker")?;
    let alert_store = store.clone();
    let alert_client = client.clone();
    let alert_config = config.clone();
    tokio::spawn(async move {
        run_alert_evaluation_loop(alert_store, Some(alert_client), alert_config).await;
    });
    ensure_telemetry_stream(&client, &config).await?;
    ensure_telemetry_consumer(&client, &config).await?;
    let action_dispatch_store = store.clone();
    let action_dispatch_client = client.clone();
    let action_dispatch_config = config.clone();
    tokio::spawn(async move {
        if let Err(error) = run_action_dispatch_loop(
            action_dispatch_store,
            action_dispatch_client,
            action_dispatch_config,
        )
        .await
        {
            tracing::error!(?error, "action dispatcher stopped");
        }
    });
    let mut subscription = client
        .subscribe(
            &config.telemetry_delivery_subject,
            Some(&config.telemetry_queue_group),
        )
        .await
        .with_context(|| {
            format!(
                "failed to subscribe worker to {}",
                config.telemetry_delivery_subject
            )
        })?;
    info!(
        subject = %config.telemetry_subject,
        delivery_subject = %config.telemetry_delivery_subject,
        stream = %config.telemetry_stream,
        queue = %config.telemetry_queue_group,
        batch_size = config.telemetry_batch_size,
        "worker JetStream telemetry consumer ready"
    );

    let mut pending = Vec::new();
    let mut batch_started_at = Instant::now();
    loop {
        let message = if pending.is_empty() {
            subscription.next_message().await?
        } else {
            match tokio::time::timeout(
                Duration::from_millis(config.telemetry_batch_window_ms),
                subscription.next_message(),
            )
            .await
            {
                Ok(message) => message?,
                Err(_) => {
                    flush_pending_telemetry_batch(&store, &client, &mut pending).await?;
                    batch_started_at = Instant::now();
                    continue;
                }
            }
        };
        match serde_json::from_slice::<TelemetryIngestEnvelope>(&message.payload) {
            Ok(envelope) => {
                pending.push(PendingTelemetryMessage {
                    envelope,
                    ack_subject: message.reply,
                });
                if pending.len() >= config.telemetry_batch_size
                    || batch_started_at.elapsed()
                        >= Duration::from_millis(config.telemetry_batch_window_ms)
                {
                    flush_pending_telemetry_batch(&store, &client, &mut pending).await?;
                    batch_started_at = Instant::now();
                }
            }
            Err(error) => {
                warn!(
                    subject = %message.subject,
                    %error,
                    "dead-lettering invalid telemetry envelope"
                );
                client
                    .publish_jetstream(
                        &config.telemetry_dead_letter_subject,
                        &message.payload,
                        Duration::from_secs(5),
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "failed to publish dead-letter telemetry to {}",
                            config.telemetry_dead_letter_subject
                        )
                    })?;
                if let Some(ack_subject) = message.reply {
                    client
                        .ack(&ack_subject)
                        .await
                        .context("failed to ack invalid telemetry envelope")?;
                }
            }
        }
    }
}

async fn ensure_telemetry_stream(client: &NatsClient, config: &WorkerConfig) -> anyhow::Result<()> {
    let payload = serde_json::json!({
        "name": config.telemetry_stream,
        "subjects": [config.telemetry_subject, config.telemetry_dead_letter_subject],
        "retention": "limits",
        "storage": "file",
        "discard": "old",
        "max_msgs": -1,
        "max_bytes": -1,
        "max_age": 0,
        "max_msg_size": -1
    });
    let api_subject = format!("$JS.API.STREAM.CREATE.{}", config.telemetry_stream);
    let response = client
        .request(
            &api_subject,
            payload.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .await
        .with_context(|| {
            format!(
                "failed to ensure JetStream stream {}",
                config.telemetry_stream
            )
        })?;
    let response_json = serde_json::from_slice::<serde_json::Value>(&response.payload)
        .context("JetStream stream create response was not JSON")?;
    if let Some(error) = response_json.get("error") {
        let description = error
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown JetStream error");
        if !description.contains("already in use") && !description.contains("already exists") {
            bail!("JetStream stream create failed: {description}");
        }
    }
    verify_stream_subjects(
        client,
        &config.telemetry_stream,
        &[
            config.telemetry_subject.as_str(),
            config.telemetry_dead_letter_subject.as_str(),
        ],
    )
    .await?;
    info!(
        stream = %config.telemetry_stream,
        subject = %config.telemetry_subject,
        "JetStream telemetry stream ready"
    );
    Ok(())
}

async fn ensure_telemetry_consumer(
    client: &NatsClient,
    config: &WorkerConfig,
) -> anyhow::Result<()> {
    let payload = serde_json::json!({
        "stream_name": config.telemetry_stream,
        "config": {
            "durable_name": config.telemetry_queue_group,
            "deliver_subject": config.telemetry_delivery_subject,
            "deliver_group": config.telemetry_queue_group,
            "filter_subject": config.telemetry_subject,
            "deliver_policy": "all",
            "ack_policy": "explicit",
            "max_ack_pending": config.telemetry_batch_size.max(config.action_claim_limit).max(1024),
        }
    });
    let api_subject = format!(
        "$JS.API.CONSUMER.DURABLE.CREATE.{}.{}",
        config.telemetry_stream, config.telemetry_queue_group
    );
    let response = client
        .request(
            &api_subject,
            payload.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .await
        .with_context(|| {
            format!(
                "failed to ensure JetStream consumer {} on {}",
                config.telemetry_queue_group, config.telemetry_stream
            )
        })?;
    let response_json = serde_json::from_slice::<serde_json::Value>(&response.payload)
        .context("JetStream consumer create response was not JSON")?;
    if let Some(error) = response_json.get("error") {
        let description = error
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown JetStream error");
        let already_exists =
            description.contains("already in use") || description.contains("already exists");
        if !already_exists {
            bail!("JetStream consumer create failed: {description}");
        }
    }
    verify_consumer_config(client, config).await?;
    info!(
        stream = %config.telemetry_stream,
        durable = %config.telemetry_queue_group,
        delivery_subject = %config.telemetry_delivery_subject,
        num_pending = response_json.get("num_pending").and_then(serde_json::Value::as_u64).unwrap_or(0),
        num_ack_pending = response_json.get("num_ack_pending").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "JetStream telemetry consumer ready"
    );
    Ok(())
}

async fn verify_stream_subjects(
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
    let response_json = serde_json::from_slice::<serde_json::Value>(&response.payload)
        .context("JetStream stream info response was not JSON")?;
    fail_on_jetstream_error(&response_json, "stream info")?;
    let subjects = response_json
        .pointer("/config/subjects")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("JetStream stream info missing config.subjects"))?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    for expected in expected_subjects {
        if !subjects.iter().any(|subject| subject == expected) {
            bail!("JetStream stream {stream} is missing expected subject {expected}");
        }
    }
    Ok(())
}

async fn verify_consumer_config(client: &NatsClient, config: &WorkerConfig) -> anyhow::Result<()> {
    let response = client
        .request(
            &format!(
                "$JS.API.CONSUMER.INFO.{}.{}",
                config.telemetry_stream, config.telemetry_queue_group
            ),
            b"{}",
            Duration::from_secs(5),
        )
        .await
        .with_context(|| {
            format!(
                "failed to read JetStream consumer info for {} on {}",
                config.telemetry_queue_group, config.telemetry_stream
            )
        })?;
    let response_json = serde_json::from_slice::<serde_json::Value>(&response.payload)
        .context("JetStream consumer info response was not JSON")?;
    fail_on_jetstream_error(&response_json, "consumer info")?;
    let consumer_config = response_json
        .get("config")
        .ok_or_else(|| anyhow::anyhow!("JetStream consumer info missing config"))?;
    validate_consumer_config_fields(consumer_config, config)
}

fn validate_consumer_config_fields(
    consumer_config: &serde_json::Value,
    config: &WorkerConfig,
) -> anyhow::Result<()> {
    let expected = [
        ("durable_name", config.telemetry_queue_group.as_str()),
        (
            "deliver_subject",
            config.telemetry_delivery_subject.as_str(),
        ),
        ("deliver_group", config.telemetry_queue_group.as_str()),
        ("filter_subject", config.telemetry_subject.as_str()),
    ];
    for (field, value) in expected {
        let actual = consumer_config
            .get(field)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if actual != value {
            bail!(
                "JetStream consumer {} mismatch: expected {value}, got {actual}",
                field
            );
        }
    }
    let ack_policy = consumer_config.get("ack_policy");
    let explicit_ack = ack_policy
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| value == "explicit")
        || ack_policy
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|value| value == 2);
    if !explicit_ack {
        bail!("JetStream consumer ack_policy mismatch: expected explicit");
    }
    let max_ack_pending = consumer_config
        .get("max_ack_pending")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("JetStream consumer missing max_ack_pending"))?;
    let min_ack_pending = config.telemetry_batch_size.max(config.action_claim_limit) as u64;
    if max_ack_pending < min_ack_pending {
        bail!(
            "JetStream consumer max_ack_pending is too low: expected at least {min_ack_pending}, got {max_ack_pending}"
        );
    }
    Ok(())
}

fn fail_on_jetstream_error(value: &serde_json::Value, context: &str) -> anyhow::Result<()> {
    if let Some(error) = value.get("error") {
        let description = error
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown JetStream error");
        bail!("JetStream {context} failed: {description}");
    }
    Ok(())
}

async fn heartbeat_loop() -> anyhow::Result<()> {
    loop {
        debug!("worker heartbeat: NATS telemetry/action loops attach when configured");
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

#[cfg(test)]
async fn flush_telemetry_batch(
    store: &Store,
    pending: &mut Vec<TelemetryIngestEnvelope>,
) -> anyhow::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }

    let batch_size = pending.len();
    let point_count = pending
        .iter()
        .map(|envelope| envelope.points.len())
        .sum::<usize>();
    let started_at = Instant::now();
    let mut written = 0usize;
    for envelope in pending.drain(..) {
        written += write_telemetry_envelope(store, envelope).await?;
    }
    info!(
        batch_size,
        point_count,
        written,
        write_duration_ms = started_at.elapsed().as_millis(),
        "telemetry batch written"
    );
    Ok(())
}

async fn flush_pending_telemetry_batch(
    store: &Store,
    client: &NatsClient,
    pending: &mut Vec<PendingTelemetryMessage>,
) -> anyhow::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }

    let batch = std::mem::take(pending);
    let batch_size = batch.len();
    let point_count = batch
        .iter()
        .map(|message| message.envelope.points.len())
        .sum::<usize>();
    let ack_subjects = batch
        .iter()
        .filter_map(|message| message.ack_subject.clone())
        .collect::<Vec<_>>();
    let started_at = Instant::now();
    let mut written = 0usize;
    for message in batch {
        written += write_telemetry_envelope(store, message.envelope).await?;
    }
    for ack_subject in ack_subjects {
        client
            .ack(&ack_subject)
            .await
            .context("failed to ack telemetry envelope")?;
    }
    info!(
        batch_size,
        point_count,
        written,
        write_duration_ms = started_at.elapsed().as_millis(),
        "telemetry batch written and acked"
    );
    Ok(())
}

async fn run_action_dispatch_loop(
    store: Store,
    client: NatsClient,
    config: WorkerConfig,
) -> anyhow::Result<()> {
    loop {
        match dispatch_action_targets_once(&store, &client, &config).await {
            Ok(dispatched) if dispatched > 0 => {
                info!(dispatched, "action targets dispatched");
            }
            Ok(_) => {}
            Err(error) => {
                warn!(?error, "action dispatch pass failed; retrying");
            }
        }
        match timeout_action_targets_once(&store, &config).await {
            Ok(timed_out) if timed_out > 0 => {
                warn!(timed_out, "action targets timed out");
            }
            Ok(_) => {}
            Err(error) => {
                warn!(?error, "action timeout sweep failed; retrying");
            }
        }
        tokio::time::sleep(Duration::from_millis(config.action_dispatch_interval_ms)).await;
    }
}

async fn dispatch_action_targets_once(
    store: &Store,
    client: &NatsClient,
    config: &WorkerConfig,
) -> anyhow::Result<usize> {
    let targets = store
        .claim_queued_action_targets(config.action_claim_limit)
        .await?;
    let mut dispatched = 0usize;
    for target in targets {
        let envelope = command_envelope_for_target(target.clone());
        let payload = serde_json::to_vec(&envelope).context("failed to encode command envelope")?;
        match client
            .publish(&config.action_command_subject, &payload)
            .await
        {
            Ok(()) => dispatched += 1,
            Err(error) => {
                warn!(
                    ?error,
                    action_id = %target.action_id,
                    device_id = %target.device_id,
                    "failed to dispatch action target"
                );
                store
                    .transition_action_targets(ActionTargetTransition {
                        project_id: target.project_id,
                        action_id: target.action_id,
                        device_ids: Some(vec![target.device_id]),
                        allowed_source_states: vec![ActionState::Running],
                        next_state: ActionState::Queued,
                        progress: Some(0),
                        errors: Some(vec![format!("dispatch retry pending: {error}")]),
                        ts: chrono::Utc::now(),
                    })
                    .await?;
            }
        }
    }
    Ok(dispatched)
}

async fn timeout_action_targets_once(
    store: &Store,
    config: &WorkerConfig,
) -> anyhow::Result<usize> {
    let now = chrono::Utc::now();
    let timeout_seconds = i64::try_from(config.action_timeout_seconds)
        .context("WORKER_ACTION_TIMEOUT_SECONDS is too large")?;
    let older_than = now - chrono::Duration::seconds(timeout_seconds);
    let timed_out = store
        .timeout_running_action_targets(older_than, config.action_timeout_scan_limit, now)
        .await?;
    Ok(timed_out.len())
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct AlertEvaluationSummary {
    firing: usize,
    resolved: usize,
}

async fn run_alert_evaluation_loop(store: Store, client: Option<NatsClient>, config: WorkerConfig) {
    loop {
        match evaluate_alerts_once(&store, client.as_ref(), &config).await {
            Ok(summary) if summary.firing > 0 || summary.resolved > 0 => {
                info!(
                    firing = summary.firing,
                    resolved = summary.resolved,
                    "alert evaluation pass completed"
                );
            }
            Ok(_) => {}
            Err(error) => {
                warn!(?error, "alert evaluation pass failed; retrying");
            }
        }
        tokio::time::sleep(Duration::from_millis(config.alert_scan_interval_ms)).await;
    }
}

async fn evaluate_alerts_once(
    store: &Store,
    client: Option<&NatsClient>,
    config: &WorkerConfig,
) -> anyhow::Result<AlertEvaluationSummary> {
    let mut summary = AlertEvaluationSummary::default();
    for rule in store.list_enabled_alerts().await? {
        let rule_summary = match rule.kind {
            AlertKind::Offline => evaluate_offline_alert(store, client, config, &rule).await?,
            AlertKind::Threshold | AlertKind::WindowAggregation => {
                evaluate_threshold_alert(store, client, config, &rule).await?
            }
        };
        summary.firing += rule_summary.firing;
        summary.resolved += rule_summary.resolved;
    }
    Ok(summary)
}

async fn evaluate_offline_alert(
    store: &Store,
    client: Option<&NatsClient>,
    config: &WorkerConfig,
    rule: &AlertRule,
) -> anyhow::Result<AlertEvaluationSummary> {
    let mut summary = AlertEvaluationSummary::default();
    let now = chrono::Utc::now();
    let offline_after_seconds = expression_i64(
        &rule.expression,
        "offline_after_seconds",
        config.alert_default_offline_after_seconds,
    )
    .max(1);
    let cutoff = now - chrono::Duration::seconds(offline_after_seconds);
    for device in target_devices(store, rule).await? {
        let dedupe_key = format!("offline:{}", device.id);
        let is_offline = device
            .last_seen_at
            .is_none_or(|last_seen| last_seen < cutoff)
            && !matches!(device.status, excalibur_domain::DeviceStatus::Disabled);
        if is_offline {
            let observed_age = device
                .last_seen_at
                .map(|last_seen| (now - last_seen).num_seconds() as f64);
            let event = AlertEvent::firing(NewAlertEvent {
                project_id: rule.project_id,
                alert_rule_id: rule.id,
                device_id: Some(device.id),
                dedupe_key,
                message: format!(
                    "{} has not checked in for at least {} seconds",
                    device.name, offline_after_seconds
                ),
                observed_value: observed_age,
                threshold: Some(offline_after_seconds as f64),
                ts: now,
            });
            let event = store.upsert_firing_alert_event(event).await?;
            summary.firing += 1;
            maybe_notify_alert_event(store, client, config, &event, false).await?;
        } else if let Some(event) = store
            .resolve_alert_event(rule.project_id, rule.id, &dedupe_key, now)
            .await?
        {
            summary.resolved += 1;
            maybe_notify_alert_event(store, client, config, &event, true).await?;
        }
    }
    Ok(summary)
}

async fn evaluate_threshold_alert(
    store: &Store,
    client: Option<&NatsClient>,
    config: &WorkerConfig,
    rule: &AlertRule,
) -> anyhow::Result<AlertEvaluationSummary> {
    let mut summary = AlertEvaluationSummary::default();
    let Some(stream) = expression_string(&rule.expression, "stream") else {
        warn!(alert_id = %rule.id, "threshold alert missing stream");
        return Ok(summary);
    };
    let Some(field) = expression_string(&rule.expression, "field") else {
        warn!(alert_id = %rule.id, "threshold alert missing field");
        return Ok(summary);
    };
    let Some(threshold) = expression_f64(&rule.expression, "threshold") else {
        warn!(alert_id = %rule.id, "threshold alert missing threshold");
        return Ok(summary);
    };
    let op = expression_string(&rule.expression, "op").unwrap_or_else(|| "gt".to_owned());
    let aggregate = expression_string(&rule.expression, "aggregate").unwrap_or_else(|| {
        if matches!(rule.kind, AlertKind::WindowAggregation) {
            "avg".to_owned()
        } else {
            "last".to_owned()
        }
    });
    let window_seconds = expression_i64(
        &rule.expression,
        "window_seconds",
        config.alert_default_window_seconds,
    )
    .max(1);
    let now = chrono::Utc::now();
    let window_start = now - chrono::Duration::seconds(window_seconds);

    for device in target_devices(store, rule).await? {
        let dedupe_key = format!("threshold:{}:{}:{}:{}", rule.id, device.id, stream, field);
        let rows = store
            .query_telemetry(rule.project_id, Some(device.id), Some(&stream), 1000)
            .await?;
        let values = rows
            .into_iter()
            .filter(|point| point.ts >= window_start)
            .filter_map(|point| {
                numeric_field(&point.payload, &field).map(|value| (point.ts, value))
            })
            .collect::<Vec<_>>();
        let observed_value = match aggregate.as_str() {
            "avg" => (!values.is_empty())
                .then(|| values.iter().map(|(_, value)| *value).sum::<f64>() / values.len() as f64),
            "max" => values.iter().map(|(_, value)| *value).reduce(f64::max),
            "min" => values.iter().map(|(_, value)| *value).reduce(f64::min),
            _ => values
                .iter()
                .max_by_key(|(ts, _)| *ts)
                .map(|(_, value)| *value),
        };
        let Some(observed_value) = observed_value else {
            if let Some(event) = store
                .resolve_alert_event(rule.project_id, rule.id, &dedupe_key, now)
                .await?
            {
                summary.resolved += 1;
                maybe_notify_alert_event(store, client, config, &event, true).await?;
            }
            continue;
        };
        if compare_threshold(observed_value, threshold, &op) {
            let event = AlertEvent::firing(NewAlertEvent {
                project_id: rule.project_id,
                alert_rule_id: rule.id,
                device_id: Some(device.id),
                dedupe_key,
                message: format!(
                    "{} {}.{} {} {} (observed {})",
                    device.name, stream, field, op, threshold, observed_value
                ),
                observed_value: Some(observed_value),
                threshold: Some(threshold),
                ts: now,
            });
            let event = store.upsert_firing_alert_event(event).await?;
            summary.firing += 1;
            maybe_notify_alert_event(store, client, config, &event, false).await?;
        } else if let Some(event) = store
            .resolve_alert_event(rule.project_id, rule.id, &dedupe_key, now)
            .await?
        {
            summary.resolved += 1;
            maybe_notify_alert_event(store, client, config, &event, true).await?;
        }
    }
    Ok(summary)
}

async fn target_devices(store: &Store, rule: &AlertRule) -> anyhow::Result<Vec<Device>> {
    if let Some(device_id) = expression_id(&rule.expression, "device_id") {
        return Ok(vec![store.get_device(rule.project_id, device_id).await?]);
    }
    Ok(store.list_devices(rule.project_id).await?)
}

async fn maybe_notify_alert_event(
    store: &Store,
    client: Option<&NatsClient>,
    config: &WorkerConfig,
    event: &AlertEvent,
    force: bool,
) -> anyhow::Result<()> {
    if !force && event.notification_attempts > 0 && event.last_notification_error.is_none() {
        return Ok(());
    }
    let Some(client) = client else {
        return Ok(());
    };
    let payload = serde_json::json!({
        "event_id": event.id,
        "project_id": event.project_id,
        "alert_rule_id": event.alert_rule_id,
        "device_id": event.device_id,
        "state": event.state,
        "message": event.message,
        "observed_value": event.observed_value,
        "threshold": event.threshold,
        "last_seen_at": event.last_seen_at,
    });
    let result = client
        .publish(
            &config.alert_notification_subject,
            payload.to_string().as_bytes(),
        )
        .await;
    let error = result.err().map(|error| error.to_string());
    store
        .record_alert_notification_attempt(event.project_id, event.id, error, chrono::Utc::now())
        .await?;
    Ok(())
}

fn expression_string(expression: &Value, field: &str) -> Option<String> {
    expression
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn expression_f64(expression: &Value, field: &str) -> Option<f64> {
    expression.get(field).and_then(Value::as_f64)
}

fn expression_i64(expression: &Value, field: &str, default: i64) -> i64 {
    expression
        .get(field)
        .and_then(Value::as_i64)
        .unwrap_or(default)
}

fn expression_id(expression: &Value, field: &str) -> Option<Id> {
    expression
        .get(field)
        .and_then(Value::as_str)
        .and_then(|value| Id::parse_str(value).ok())
}

fn numeric_field(payload: &Value, field: &str) -> Option<f64> {
    payload.get(field).and_then(Value::as_f64)
}

fn compare_threshold(observed: f64, threshold: f64, op: &str) -> bool {
    match op {
        "gte" | "ge" => observed >= threshold,
        "lt" => observed < threshold,
        "lte" | "le" => observed <= threshold,
        "eq" => (observed - threshold).abs() <= f64::EPSILON,
        _ => observed > threshold,
    }
}

fn command_envelope_for_target(target: ActionDispatchTarget) -> DeviceCommandEnvelope {
    DeviceCommandEnvelope {
        project_id: target.project_id,
        device_id: target.device_id,
        topic: commands_topic(target.project_id, target.device_id),
        command: command_for_action(target.action_id, target.name, target.payload),
    }
}

fn worker_config_from_env() -> anyhow::Result<WorkerConfig> {
    let database_url = std::env::var("DATABASE_URL").ok();
    let storage_backend = std::env::var("STORAGE_BACKEND").ok();
    Ok(WorkerConfig {
        storage: storage_config(storage_backend, database_url)?,
        nats_url: std::env::var("NATS_URL").ok(),
        telemetry_subject: std::env::var("WORKER_TELEMETRY_NATS_SUBJECT")
            .unwrap_or_else(|_| "excalibur.telemetry.ingest".to_owned()),
        telemetry_stream: std::env::var("WORKER_TELEMETRY_NATS_STREAM")
            .unwrap_or_else(|_| "EXCALIBUR_TELEMETRY".to_owned()),
        telemetry_delivery_subject: std::env::var("WORKER_TELEMETRY_DELIVERY_SUBJECT")
            .unwrap_or_else(|_| "excalibur.telemetry.deliver".to_owned()),
        telemetry_dead_letter_subject: std::env::var("WORKER_TELEMETRY_DEAD_LETTER_SUBJECT")
            .unwrap_or_else(|_| "excalibur.telemetry.dead_letter".to_owned()),
        telemetry_queue_group: std::env::var("WORKER_TELEMETRY_QUEUE_GROUP")
            .unwrap_or_else(|_| "excalibur-telemetry-workers".to_owned()),
        telemetry_batch_size: parse_env("WORKER_TELEMETRY_BATCH_SIZE", "256")?,
        telemetry_batch_window_ms: parse_env("WORKER_TELEMETRY_BATCH_WINDOW_MS", "1000")?,
        action_command_subject: std::env::var("WORKER_ACTION_COMMAND_SUBJECT")
            .unwrap_or_else(|_| "excalibur.commands.dispatch".to_owned()),
        action_claim_limit: parse_env("WORKER_ACTION_CLAIM_LIMIT", "100")?,
        action_dispatch_interval_ms: parse_env("WORKER_ACTION_DISPATCH_INTERVAL_MS", "1000")?,
        action_timeout_seconds: parse_env("WORKER_ACTION_TIMEOUT_SECONDS", "900")?,
        action_timeout_scan_limit: parse_env("WORKER_ACTION_TIMEOUT_SCAN_LIMIT", "100")?,
        alert_scan_interval_ms: parse_env("WORKER_ALERT_SCAN_INTERVAL_MS", "30000")?,
        alert_default_offline_after_seconds: parse_env(
            "WORKER_ALERT_DEFAULT_OFFLINE_AFTER_SECONDS",
            "300",
        )?,
        alert_default_window_seconds: parse_env("WORKER_ALERT_DEFAULT_WINDOW_SECONDS", "300")?,
        alert_notification_subject: std::env::var("WORKER_ALERT_NOTIFICATION_SUBJECT")
            .unwrap_or_else(|_| "excalibur.alerts.notifications".to_owned()),
    })
}

fn parse_env<T>(name: &str, default: &str) -> anyhow::Result<T>
where
    T: FromStr,
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
            warn!("using in-memory worker store");
            Ok(Store::memory())
        }
        StorageConfig::Sql { database_url } => {
            let pg_store = PgStore::connect(&database_url)
                .await
                .context("failed to connect SQL storage")?;
            pg_store
                .validate_schema()
                .await
                .context("SQL schema validation failed")?;
            Ok(Store::postgres(pg_store))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use excalibur_device_protocol::DeviceCommandEnvelope;
    use excalibur_device_protocol::TelemetryIngestPoint;
    use excalibur_domain::{
        ActionDispatchTarget, ActionState, AlertEventState, AlertKind, AlertRule, Device, Id, Org,
        Project, TelemetryPoint, User,
    };
    use serde_json::json;

    #[test]
    fn storage_config_defaults_to_timescale_when_database_url_exists() {
        assert_eq!(
            storage_config(None, Some("postgres://example/excalibur".to_owned())).unwrap(),
            StorageConfig::Sql {
                database_url: "postgres://example/excalibur".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn flush_telemetry_batch_preserves_store_dedupe() {
        let store = Store::memory();
        let user = store
            .create_user(User::new("worker@example.com", "Worker", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Worker Org", "worker-org"), user.id)
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
        let ts = Utc::now();
        let mut batch = vec![TelemetryIngestEnvelope {
            project_id: project.id,
            device_id: device.id,
            stream: "temperature".to_owned(),
            points: vec![
                TelemetryIngestPoint {
                    sequence: 1,
                    timestamp: ts,
                    payload: json!({ "value": 24.1 }),
                },
                TelemetryIngestPoint {
                    sequence: 1,
                    timestamp: ts + chrono::Duration::seconds(1),
                    payload: json!({ "value": 25.0 }),
                },
            ],
            received_at: Utc::now(),
        }];

        flush_telemetry_batch(&store, &mut batch).await.unwrap();

        let rows = store
            .query_telemetry(project.id, Some(device.id), Some("temperature"), 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].payload["value"], 24.1);
    }

    #[test]
    fn command_envelope_targets_device_command_topic() {
        let project_id = Id::now_v7();
        let device_id = Id::now_v7();
        let action_id = Id::now_v7();

        let envelope = command_envelope_for_target(ActionDispatchTarget {
            project_id,
            action_id,
            device_id,
            name: "diagnostics.collect".to_owned(),
            payload: json!({ "paths": ["/var/log"] }),
        });

        assert_eq!(
            envelope,
            DeviceCommandEnvelope {
                project_id,
                device_id,
                topic: commands_topic(project_id, device_id),
                command: command_for_action(
                    action_id,
                    "diagnostics.collect",
                    json!({ "paths": ["/var/log"] })
                )
            }
        );
    }

    #[tokio::test]
    async fn timeout_action_targets_once_marks_stale_running_targets() {
        let store = Store::memory();
        let user = store
            .create_user(User::new("timeout@example.com", "Timeout", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Timeout Org", "timeout-org"), user.id)
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
        let action = store
            .create_action(excalibur_domain::Action::new(
                project.id,
                vec![device.id],
                "diagnostics.collect",
                json!({ "session_id": Id::now_v7() }),
                Some(user.id),
            ))
            .await
            .unwrap();
        assert_eq!(
            store.claim_queued_action_targets(10).await.unwrap().len(),
            1
        );

        let config = WorkerConfig {
            storage: StorageConfig::Memory,
            nats_url: None,
            telemetry_subject: "excalibur.telemetry.ingest".to_owned(),
            telemetry_stream: "EXCALIBUR_TELEMETRY".to_owned(),
            telemetry_delivery_subject: "excalibur.telemetry.deliver".to_owned(),
            telemetry_dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
            telemetry_queue_group: "excalibur-telemetry-workers".to_owned(),
            telemetry_batch_size: 10,
            telemetry_batch_window_ms: 100,
            action_command_subject: "excalibur.commands.dispatch".to_owned(),
            action_claim_limit: 10,
            action_dispatch_interval_ms: 100,
            action_timeout_seconds: 0,
            action_timeout_scan_limit: 10,
            alert_scan_interval_ms: 100,
            alert_default_offline_after_seconds: 300,
            alert_default_window_seconds: 300,
            alert_notification_subject: "excalibur.alerts.notifications".to_owned(),
        };

        assert_eq!(
            timeout_action_targets_once(&store, &config).await.unwrap(),
            1
        );
        let actions = store.list_actions(project.id).await.unwrap();
        let stored = actions
            .into_iter()
            .find(|candidate| candidate.id == action.id)
            .unwrap();
        assert_eq!(stored.state, ActionState::TimedOut);
    }

    #[tokio::test]
    async fn dispatch_publish_failure_requeues_action_target_for_retry() {
        let store = Store::memory();
        let user = store
            .create_user(User::new(
                "dispatch-retry@example.com",
                "Dispatch Retry",
                "hash",
            ))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Dispatch Retry Org", "dispatch-retry"), user.id)
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
        let action = store
            .create_action(excalibur_domain::Action::new(
                project.id,
                vec![device.id],
                "diagnostics.collect",
                json!({ "session_id": Id::now_v7() }),
                Some(user.id),
            ))
            .await
            .unwrap();
        let client = NatsClient::new("nats://127.0.0.1:9", "dispatch-retry-test").unwrap();

        assert_eq!(
            dispatch_action_targets_once(&store, &client, &test_worker_config())
                .await
                .unwrap(),
            0
        );
        let retry_claim = store.claim_queued_action_targets(1).await.unwrap();
        assert_eq!(retry_claim.len(), 1);
        assert_eq!(retry_claim[0].action_id, action.id);
        assert_eq!(retry_claim[0].device_id, device.id);
    }

    fn test_worker_config() -> WorkerConfig {
        WorkerConfig {
            storage: StorageConfig::Memory,
            nats_url: None,
            telemetry_subject: "excalibur.telemetry.ingest".to_owned(),
            telemetry_stream: "EXCALIBUR_TELEMETRY".to_owned(),
            telemetry_delivery_subject: "excalibur.telemetry.deliver".to_owned(),
            telemetry_dead_letter_subject: "excalibur.telemetry.dead_letter".to_owned(),
            telemetry_queue_group: "excalibur-telemetry-workers".to_owned(),
            telemetry_batch_size: 10,
            telemetry_batch_window_ms: 100,
            action_command_subject: "excalibur.commands.dispatch".to_owned(),
            action_claim_limit: 10,
            action_dispatch_interval_ms: 100,
            action_timeout_seconds: 900,
            action_timeout_scan_limit: 10,
            alert_scan_interval_ms: 100,
            alert_default_offline_after_seconds: 300,
            alert_default_window_seconds: 300,
            alert_notification_subject: "excalibur.alerts.notifications".to_owned(),
        }
    }

    #[test]
    fn consumer_config_validation_rejects_non_explicit_ack_policy() {
        let config = test_worker_config();
        let consumer_config = json!({
            "durable_name": config.telemetry_queue_group,
            "deliver_subject": config.telemetry_delivery_subject,
            "deliver_group": config.telemetry_queue_group,
            "filter_subject": config.telemetry_subject,
            "ack_policy": "none",
            "max_ack_pending": 1024
        });

        let error = validate_consumer_config_fields(&consumer_config, &config).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("ack_policy mismatch: expected explicit")
        );
    }

    #[tokio::test]
    async fn threshold_alert_evaluation_opens_and_resolves_event() {
        let store = Store::memory();
        let user = store
            .create_user(User::new("alert-threshold@example.com", "Alert", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Alert Org", "alert-org"), user.id)
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
        let rule = store
            .create_alert(AlertRule {
                id: Id::now_v7(),
                project_id: project.id,
                name: "high temp".to_owned(),
                kind: AlertKind::Threshold,
                expression: json!({
                    "stream": "temperature",
                    "field": "value",
                    "threshold": 80.0,
                    "op": "gt",
                    "window_seconds": 300
                }),
                enabled: true,
            })
            .await
            .unwrap();
        store
            .write_telemetry(vec![TelemetryPoint {
                project_id: project.id,
                device_id: device.id,
                stream: "temperature".to_owned(),
                sequence: 1,
                ts: Utc::now(),
                payload: json!({"value": 91.0}),
                ingested_at: Utc::now(),
            }])
            .await
            .unwrap();

        let summary = evaluate_alerts_once(&store, None, &test_worker_config())
            .await
            .unwrap();
        assert_eq!(summary.firing, 1);
        let events = store
            .list_alert_events(project.id, Some(AlertEventState::Firing))
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].alert_rule_id, rule.id);

        store
            .write_telemetry(vec![TelemetryPoint {
                project_id: project.id,
                device_id: device.id,
                stream: "temperature".to_owned(),
                sequence: 2,
                ts: Utc::now(),
                payload: json!({"value": 70.0}),
                ingested_at: Utc::now(),
            }])
            .await
            .unwrap();
        let summary = evaluate_alerts_once(&store, None, &test_worker_config())
            .await
            .unwrap();
        assert_eq!(summary.resolved, 1);
        assert_eq!(
            store
                .list_alert_events(project.id, Some(AlertEventState::Resolved))
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn offline_alert_evaluation_dedupes_open_events() {
        let store = Store::memory();
        let user = store
            .create_user(User::new("alert-offline@example.com", "Alert", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Offline Org", "offline-org"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        store
            .create_alert(AlertRule {
                id: Id::now_v7(),
                project_id: project.id,
                name: "offline".to_owned(),
                kind: AlertKind::Offline,
                expression: json!({ "offline_after_seconds": 1 }),
                enabled: true,
            })
            .await
            .unwrap();

        assert_eq!(
            evaluate_alerts_once(&store, None, &test_worker_config())
                .await
                .unwrap()
                .firing,
            1
        );
        assert_eq!(
            evaluate_alerts_once(&store, None, &test_worker_config())
                .await
                .unwrap()
                .firing,
            1
        );
        let events = store
            .list_alert_events(project.id, Some(AlertEventState::Firing))
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
    }
}
