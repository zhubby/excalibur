use chrono::Utc;
use excalibur_device_protocol::{
    ProtocolError, PublishTopic, SubscribeTopic, TelemetryIngestEnvelope, TelemetryIngestPoint,
    decode_command_status_payload, decode_telemetry_payload, parse_publish_topic,
    parse_subscribe_topic, validate_device_scope,
};
use excalibur_domain::{ActionStatusUpdate, DeviceStatus, Id, TelemetryPoint};
use excalibur_storage::{Store, StoreError, map_terminal_action_state};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedDevice {
    pub project_id: Id,
    pub device_id: Id,
    pub status: DeviceStatus,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IngestError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("device is not allowed to connect")]
    DeviceDisabled,
}

pub async fn authenticate_device_certificate_fingerprint(
    store: &Store,
    fingerprint_sha256: &str,
) -> Result<AuthenticatedDevice, IngestError> {
    let device = store
        .get_active_device_by_certificate_fingerprint(fingerprint_sha256)
        .await?;
    if matches!(device.status, DeviceStatus::Disabled) {
        return Err(IngestError::DeviceDisabled);
    }

    Ok(AuthenticatedDevice {
        project_id: device.project_id,
        device_id: device.id,
        status: device.status,
    })
}

pub fn authorize_publish(
    topic: &str,
    device: AuthenticatedDevice,
) -> Result<PublishTopic, IngestError> {
    if matches!(device.status, DeviceStatus::Disabled) {
        return Err(IngestError::DeviceDisabled);
    }
    let parsed = parse_publish_topic(topic)?;
    match &parsed {
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
        } => {
            validate_device_scope(*project_id, *device_id, device.project_id, device.device_id)?;
        }
    }
    Ok(parsed)
}

pub fn authorize_subscribe(
    topic: &str,
    device: AuthenticatedDevice,
) -> Result<SubscribeTopic, IngestError> {
    if matches!(device.status, DeviceStatus::Disabled) {
        return Err(IngestError::DeviceDisabled);
    }
    let parsed = parse_subscribe_topic(topic)?;
    match &parsed {
        SubscribeTopic::Commands {
            project_id,
            device_id,
        } => {
            validate_device_scope(*project_id, *device_id, device.project_id, device.device_id)?;
        }
    }
    Ok(parsed)
}

pub async fn ingest_publish(
    store: &Store,
    topic: &str,
    payload: Value,
    device: AuthenticatedDevice,
) -> Result<usize, IngestError> {
    match authorize_publish(topic, device.clone())? {
        PublishTopic::Telemetry { .. } => {
            let envelope = telemetry_envelope_from_publish(topic, payload, device)?;
            write_telemetry_envelope(store, envelope).await
        }
        PublishTopic::Shadow {
            project_id,
            device_id,
        } => {
            store.update_shadow(project_id, device_id, payload).await?;
            Ok(1)
        }
        PublishTopic::CommandStatus {
            project_id,
            device_id,
        } => {
            let updates = decode_command_status_payload(payload)?;
            let count = updates.len();
            for update in updates {
                store
                    .update_action_status(ActionStatusUpdate {
                        project_id,
                        action_id: update.action_id,
                        device_id,
                        state: map_terminal_action_state(&update.state),
                        progress: update.progress,
                        errors: update.errors,
                        ts: Utc::now(),
                    })
                    .await?;
            }
            store.touch_device_online(project_id, device_id).await?;
            Ok(count)
        }
    }
}

pub fn telemetry_envelope_from_publish(
    topic: &str,
    payload: Value,
    device: AuthenticatedDevice,
) -> Result<TelemetryIngestEnvelope, IngestError> {
    let PublishTopic::Telemetry {
        project_id,
        device_id,
        stream,
    } = authorize_publish(topic, device)?
    else {
        return Err(IngestError::Protocol(ProtocolError::InvalidTopic));
    };
    let records = decode_telemetry_payload(payload)?;
    let points = records
        .into_iter()
        .map(|record| TelemetryIngestPoint {
            sequence: record.sequence,
            timestamp: record.timestamp,
            payload: Value::Object(record.fields),
        })
        .collect();

    Ok(TelemetryIngestEnvelope {
        project_id,
        device_id,
        stream,
        points,
        received_at: Utc::now(),
    })
}

pub async fn write_telemetry_envelope(
    store: &Store,
    envelope: TelemetryIngestEnvelope,
) -> Result<usize, IngestError> {
    let points = envelope
        .points
        .into_iter()
        .map(|point| TelemetryPoint {
            project_id: envelope.project_id,
            device_id: envelope.device_id,
            stream: envelope.stream.clone(),
            sequence: point.sequence,
            ts: point.timestamp,
            payload: point.payload,
            ingested_at: envelope.received_at,
        })
        .collect();
    store
        .touch_device_online(envelope.project_id, envelope.device_id)
        .await?;
    store
        .write_telemetry(points)
        .await
        .map_err(IngestError::Store)
}

#[cfg(feature = "rumqttd-runtime")]
pub mod rumqttd_adapter {
    //! Placeholder for the production rumqttd hook implementation.
    //!
    //! The tested ACL and ingest functions in this crate are intentionally
    //! broker-agnostic. The rumqttd runtime adapter should call those functions
    //! from connect, publish, and subscribe hooks.
}

#[cfg(test)]
mod tests {
    use super::*;
    use excalibur_device_protocol::{
        command_status_topic, commands_topic, shadow_topic, telemetry_topic,
    };
    use excalibur_domain::{
        Action, ActionState, CertificateStatus, Device, DeviceCertificate, Org, Project, User,
    };
    use excalibur_storage::PgStore;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn acl_rejects_cross_tenant_publish() {
        let project_id = Uuid::now_v7();
        let device_id = Uuid::now_v7();
        let other_project = Uuid::now_v7();
        let topic = telemetry_topic(other_project, device_id, "temperature");

        let error = authorize_publish(
            &topic,
            AuthenticatedDevice {
                project_id,
                device_id,
                status: DeviceStatus::Online,
            },
        )
        .unwrap_err();

        assert_eq!(error, IngestError::Protocol(ProtocolError::ScopeMismatch));
    }

    #[test]
    fn acl_allows_command_subscription_for_own_device() {
        let project_id = Uuid::now_v7();
        let device_id = Uuid::now_v7();
        let topic = commands_topic(project_id, device_id);

        assert!(
            authorize_subscribe(
                &topic,
                AuthenticatedDevice {
                    project_id,
                    device_id,
                    status: DeviceStatus::Online,
                }
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn certificate_fingerprint_authenticates_active_device() {
        let store = Store::memory();
        let user = store
            .create_user(User::new("cert-device@example.com", "Cert Device", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Cert Org", "cert-org"), user.id)
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
        store
            .create_device_certificate(DeviceCertificate::new(
                project.id,
                device.id,
                "a".repeat(64),
                Utc::now() + chrono::Duration::days(1),
            ))
            .await
            .unwrap();

        let authenticated = authenticate_device_certificate_fingerprint(&store, &"a".repeat(64))
            .await
            .unwrap();

        assert_eq!(authenticated.project_id, project.id);
        assert_eq!(authenticated.device_id, device.id);
    }

    #[tokio::test]
    async fn certificate_fingerprint_rejects_revoked_certificate() {
        let store = Store::memory();
        let user = store
            .create_user(User::new(
                "revoked-device@example.com",
                "Revoked Device",
                "hash",
            ))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Revoked Org", "revoked-org"), user.id)
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
        let mut certificate = DeviceCertificate::new(
            project.id,
            device.id,
            "b".repeat(64),
            Utc::now() + chrono::Duration::days(1),
        );
        certificate.status = CertificateStatus::Revoked;
        store.create_device_certificate(certificate).await.unwrap();

        assert!(matches!(
            authenticate_device_certificate_fingerprint(&store, &"b".repeat(64))
                .await
                .unwrap_err(),
            IngestError::Store(StoreError::NotFound("certificate"))
        ));
    }

    #[tokio::test]
    async fn command_status_publish_updates_target_action() {
        let store = Store::memory();
        let user = store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Acme", "acme"), user.id)
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
            .create_action(Action::new(
                project.id,
                vec![device.id],
                "ota",
                json!({ "version": "1.0.0" }),
                Some(user.id),
            ))
            .await
            .unwrap();
        let claimed = store.claim_queued_action_targets(1).await.unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].action_id, action.id);

        let count = ingest_publish(
            &store,
            &command_status_topic(project.id, device.id),
            json!([
                {
                    "action_id": action.id,
                    "state": "completed",
                    "progress": 100,
                    "errors": []
                }
            ]),
            AuthenticatedDevice {
                project_id: project.id,
                device_id: device.id,
                status: DeviceStatus::Online,
            },
        )
        .await
        .unwrap();

        assert_eq!(count, 1);
        let actions = store.list_actions(project.id).await.unwrap();
        assert_eq!(actions[0].state, ActionState::Completed);
        assert_eq!(actions[0].progress, 100);
    }

    #[test]
    fn telemetry_envelope_validation_rejects_invalid_payload_before_queueing() {
        let project_id = Uuid::now_v7();
        let device_id = Uuid::now_v7();
        let authenticated = AuthenticatedDevice {
            project_id,
            device_id,
            status: DeviceStatus::Online,
        };

        let error = telemetry_envelope_from_publish(
            &telemetry_topic(project_id, device_id, "temperature"),
            json!({ "sequence": 1 }),
            authenticated,
        )
        .unwrap_err();

        assert_eq!(
            error,
            IngestError::Protocol(ProtocolError::PayloadMustBeArray)
        );
    }

    #[tokio::test]
    async fn sql_store_ingest_publish_contract_runs_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("EXCALIBUR_SQL_TEST_DATABASE_URL") else {
            eprintln!("skipping mqtt PgStore contract; EXCALIBUR_SQL_TEST_DATABASE_URL is not set");
            return;
        };

        let pg_store = PgStore::connect(&database_url).await.unwrap();
        pg_store.validate_schema().await.unwrap();
        let store = Store::postgres(pg_store);
        let suffix = Uuid::now_v7().simple().to_string();
        let user = store
            .create_user(User::new(
                format!("mqtt-sql-{suffix}@example.com"),
                "MQTT SQL",
                "hash",
            ))
            .await
            .unwrap();
        let org = store
            .create_org(
                Org::new("MQTT SQL Org", format!("mqtt-sql-org-{suffix}")),
                user.id,
            )
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(
                org.id,
                "MQTT SQL Project",
                format!("mqtt-sql-project-{suffix}"),
            ))
            .await
            .unwrap();
        let device = store
            .create_device(Device::new(project.id, "mqtt-sql-device", json!({})))
            .await
            .unwrap();
        let authenticated = AuthenticatedDevice {
            project_id: project.id,
            device_id: device.id,
            status: DeviceStatus::Online,
        };

        assert_eq!(
            ingest_publish(
                &store,
                &telemetry_topic(project.id, device.id, "temperature"),
                json!([
                    {
                        "sequence": 1,
                        "timestamp": 1710760059006i64,
                        "value": 21.5
                    }
                ]),
                authenticated.clone(),
            )
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            ingest_publish(
                &store,
                &telemetry_topic(project.id, device.id, "temperature"),
                json!([
                    {
                        "sequence": 1,
                        "timestamp": 1710760060006i64,
                        "value": 22.0
                    }
                ]),
                authenticated.clone(),
            )
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            store
                .query_telemetry(project.id, Some(device.id), Some("temperature"), 10)
                .await
                .unwrap()
                .len(),
            1
        );

        assert_eq!(
            ingest_publish(
                &store,
                &shadow_topic(project.id, device.id),
                json!({"desired": {"mode": "eco"}}),
                authenticated.clone(),
            )
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            store
                .get_device(project.id, device.id)
                .await
                .unwrap()
                .latest_shadow["desired"]["mode"],
            "eco"
        );

        let action = store
            .create_action(Action::new(
                project.id,
                vec![device.id],
                "ota",
                json!({ "version": "1.0.0" }),
                Some(user.id),
            ))
            .await
            .unwrap();
        assert_eq!(
            ingest_publish(
                &store,
                &command_status_topic(project.id, device.id),
                json!([
                    {
                        "action_id": action.id,
                        "state": "completed",
                        "progress": 100,
                        "errors": []
                    }
                ]),
                authenticated,
            )
            .await
            .unwrap(),
            1
        );
        let action = store
            .list_actions(project.id)
            .await
            .unwrap()
            .into_iter()
            .find(|stored| stored.id == action.id)
            .unwrap();
        assert_eq!(action.state, ActionState::Completed);
        assert_eq!(action.progress, 100);
    }
}
