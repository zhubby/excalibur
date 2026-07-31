use chrono::Utc;
use excalibur_domain::{
    Action, ActionState, ActionStatusUpdate, ActionTargetTransition, AlertEvent, AlertEventState,
    AlertKind, AlertRule, ApiKey, AuditLog, CertificateStatus, Dashboard, Device,
    DeviceCertificate, DeviceStatus, DiagnosticsSession, DiagnosticsSessionState, FirmwareArtifact,
    FirmwareRollout, FirmwareRolloutState, Membership, NewAlertEvent, NewFirmwareRollout, Org,
    Project, Role, StreamDefinition, StreamField, StreamFieldType, TelemetryPoint, User,
    UserSession,
};
use serde_json::json;
use uuid::Uuid;

use crate::{MemoryStore, PgStore, Store, StoreError};

#[tokio::test]
async fn enforces_project_scope_for_devices() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("ops@example.com", "Ops", "hash"))
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
    let other = store
        .create_project(Project::new(org.id, "Lab", "lab"))
        .await
        .unwrap();
    let device = store
        .create_device(Device::new(project.id, "press-1", json!({})))
        .await
        .unwrap();

    assert_eq!(
        store.get_device(project.id, device.id).await.unwrap().id,
        device.id
    );
    assert_eq!(
        store.get_device(other.id, device.id).await.unwrap_err(),
        StoreError::NotFound("device")
    );
}

#[tokio::test]
async fn writes_and_filters_telemetry() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("telemetry@example.com", "Telemetry", "hash"))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Telemetry Org", "telemetry"), user.id)
        .await
        .unwrap();
    let project = store
        .create_project(Project::new(org.id, "Plant", "plant"))
        .await
        .unwrap();
    let device = store
        .create_device(Device::new(project.id, "press-1", json!({})))
        .await
        .unwrap();
    store
        .write_telemetry(vec![TelemetryPoint {
            project_id: project.id,
            device_id: device.id,
            stream: "temperature".to_owned(),
            sequence: 1,
            ts: Utc::now(),
            payload: json!({"value": 24.1}),
            ingested_at: Utc::now(),
        }])
        .await
        .unwrap();

    let rows = store
        .query_telemetry(project.id, Some(device.id), Some("temperature"), 10)
        .await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].payload["value"], 24.1);
}

#[tokio::test]
async fn ignores_duplicate_telemetry_sequence_even_with_different_timestamp() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("dedupe@example.com", "Dedupe", "hash"))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Dedupe Org", "dedupe"), user.id)
        .await
        .unwrap();
    let project = store
        .create_project(Project::new(org.id, "Plant", "plant"))
        .await
        .unwrap();
    let device = store
        .create_device(Device::new(project.id, "press-1", json!({})))
        .await
        .unwrap();
    let ts = Utc::now();

    let written = store
        .write_telemetry(vec![
            TelemetryPoint {
                project_id: project.id,
                device_id: device.id,
                stream: "temperature".to_owned(),
                sequence: 1,
                ts,
                payload: json!({"value": 24.1}),
                ingested_at: Utc::now(),
            },
            TelemetryPoint {
                project_id: project.id,
                device_id: device.id,
                stream: "temperature".to_owned(),
                sequence: 1,
                ts: ts + chrono::Duration::seconds(1),
                payload: json!({"value": 25.0}),
                ingested_at: Utc::now(),
            },
        ])
        .await
        .unwrap();

    let rows = store
        .query_telemetry(project.id, Some(device.id), Some("temperature"), 10)
        .await;
    assert_eq!(written, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].payload["value"], 24.1);
}

#[tokio::test]
async fn aggregates_telemetry_by_time_bucket() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("aggregate@example.com", "Aggregate", "hash"))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Aggregate Org", "aggregate"), user.id)
        .await
        .unwrap();
    let project = store
        .create_project(Project::new(org.id, "Plant", "plant"))
        .await
        .unwrap();
    let device = store
        .create_device(Device::new(project.id, "press-1", json!({})))
        .await
        .unwrap();
    let base = chrono::DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
    store
        .write_telemetry(vec![
            TelemetryPoint {
                project_id: project.id,
                device_id: device.id,
                stream: "temperature".to_owned(),
                sequence: 1,
                ts: base,
                payload: json!({"value": 20.0}),
                ingested_at: Utc::now(),
            },
            TelemetryPoint {
                project_id: project.id,
                device_id: device.id,
                stream: "temperature".to_owned(),
                sequence: 2,
                ts: base + chrono::Duration::seconds(10),
                payload: json!({"value": 30.0}),
                ingested_at: Utc::now(),
            },
        ])
        .await
        .unwrap();

    let buckets = store
        .aggregate_telemetry(
            project.id,
            Some(device.id),
            "temperature",
            Some("value"),
            base - chrono::Duration::seconds(1),
            base + chrono::Duration::seconds(60),
            60,
            10,
        )
        .await;

    assert_eq!(buckets.len(), 1);
    assert_eq!(buckets[0].count, 2);
    assert_eq!(buckets[0].min, Some(20.0));
    assert_eq!(buckets[0].max, Some(30.0));
    assert_eq!(buckets[0].avg, Some(25.0));
    assert_eq!(buckets[0].last, Some(30.0));
}

#[tokio::test]
async fn stores_stream_definitions() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("owner@example.com", "Owner", "hash"))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Fleet", "fleet"), user.id)
        .await
        .unwrap();
    let project = store
        .create_project(Project::new(org.id, "EV", "ev"))
        .await
        .unwrap();
    let stream = store
        .create_stream(StreamDefinition::new(
            project.id,
            "battery",
            vec![StreamField {
                name: "voltage".to_owned(),
                field_type: StreamFieldType::Float,
                required: true,
            }],
        ))
        .await
        .unwrap();

    assert_eq!(store.list_streams(project.id).await, vec![stream]);
}

#[tokio::test]
async fn action_status_requires_action_and_device_project_scope() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("actions@example.com", "Actions", "hash"))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Actions Org", "actions"), user.id)
        .await
        .unwrap();
    let project = store
        .create_project(Project::new(org.id, "Factory", "factory"))
        .await
        .unwrap();
    let other_project = store
        .create_project(Project::new(org.id, "Lab", "lab"))
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

    let error = store
        .update_action_status(ActionStatusUpdate {
            project_id: other_project.id,
            action_id: action.id,
            device_id: device.id,
            state: ActionState::Completed,
            progress: 100,
            errors: Vec::new(),
            ts: Utc::now(),
        })
        .await
        .unwrap_err();

    assert_eq!(error, StoreError::NotFound("action"));
}

#[tokio::test]
async fn aggregates_multi_target_action_status() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new(
            "batch-actions@example.com",
            "Batch Actions",
            "hash",
        ))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Batch Actions Org", "batch-actions"), user.id)
        .await
        .unwrap();
    let project = store
        .create_project(Project::new(org.id, "Factory", "factory"))
        .await
        .unwrap();
    let first_device = store
        .create_device(Device::new(project.id, "press-1", json!({})))
        .await
        .unwrap();
    let second_device = store
        .create_device(Device::new(project.id, "press-2", json!({})))
        .await
        .unwrap();
    let action = store
        .create_action(Action::new(
            project.id,
            vec![first_device.id, second_device.id],
            "ota.install",
            json!({ "version": "1.0.0" }),
            Some(user.id),
        ))
        .await
        .unwrap();
    let claimed = store.claim_queued_action_targets(2).await.unwrap();
    assert_eq!(claimed.len(), 2);
    assert!(claimed.iter().all(|target| target.action_id == action.id));

    let partial = store
        .update_action_status(ActionStatusUpdate {
            project_id: project.id,
            action_id: action.id,
            device_id: first_device.id,
            state: ActionState::Completed,
            progress: 100,
            errors: Vec::new(),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(partial.state, ActionState::Running);
    assert_eq!(partial.progress, 50);

    let completed = store
        .update_action_status(ActionStatusUpdate {
            project_id: project.id,
            action_id: action.id,
            device_id: second_device.id,
            state: ActionState::Completed,
            progress: 100,
            errors: Vec::new(),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(completed.state, ActionState::Completed);
    assert_eq!(completed.progress, 100);
}

#[tokio::test]
async fn claims_queued_action_targets_once_and_marks_running() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new(
            "claim-actions@example.com",
            "Claim Actions",
            "hash",
        ))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Claim Actions Org", "claim-actions"), user.id)
        .await
        .unwrap();
    let project = store
        .create_project(Project::new(org.id, "Factory", "factory"))
        .await
        .unwrap();
    let first_device = store
        .create_device(Device::new(project.id, "press-1", json!({})))
        .await
        .unwrap();
    let second_device = store
        .create_device(Device::new(project.id, "press-2", json!({})))
        .await
        .unwrap();
    let action = store
        .create_action(Action::new(
            project.id,
            vec![first_device.id, second_device.id],
            "diagnostics.collect",
            json!({ "session_id": Uuid::now_v7() }),
            Some(user.id),
        ))
        .await
        .unwrap();

    let claimed = store.claim_queued_action_targets(10).await.unwrap();
    assert_eq!(claimed.len(), 2);
    assert!(claimed.iter().all(|target| target.action_id == action.id));

    let actions = store.list_actions(project.id).await;
    assert_eq!(actions[0].state, ActionState::Running);
    assert!(
        store
            .claim_queued_action_targets(10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn claim_queued_action_targets_uses_stable_fifo_order() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new(
            "ordered-actions@example.com",
            "Ordered Actions",
            "hash",
        ))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Ordered Actions Org", "ordered-actions"), user.id)
        .await
        .unwrap();
    let project = store
        .create_project(Project::new(org.id, "Factory", "factory"))
        .await
        .unwrap();
    let first_device = store
        .create_device(Device::new(project.id, "press-1", json!({})))
        .await
        .unwrap();
    let second_device = store
        .create_device(Device::new(project.id, "press-2", json!({})))
        .await
        .unwrap();
    let base_ts = Utc::now() - chrono::Duration::minutes(5);
    let mut newer_action = Action::new(
        project.id,
        vec![first_device.id],
        "diagnostics.collect",
        json!({ "session_id": Uuid::now_v7() }),
        Some(user.id),
    );
    newer_action.updated_at = base_ts + chrono::Duration::seconds(1);
    let newer_action = store.create_action(newer_action).await.unwrap();
    let mut older_action = Action::new(
        project.id,
        vec![second_device.id],
        "diagnostics.collect",
        json!({ "session_id": Uuid::now_v7() }),
        Some(user.id),
    );
    older_action.updated_at = base_ts;
    let older_action = store.create_action(older_action).await.unwrap();

    let first_claim = store.claim_queued_action_targets(1).await.unwrap();
    assert_eq!(first_claim.len(), 1);
    assert_eq!(first_claim[0].action_id, older_action.id);
    assert_eq!(first_claim[0].device_id, second_device.id);

    let second_claim = store.claim_queued_action_targets(1).await.unwrap();
    assert_eq!(second_claim.len(), 1);
    assert_eq!(second_claim[0].action_id, newer_action.id);
    assert_eq!(second_claim[0].device_id, first_device.id);
}

#[tokio::test]
async fn device_status_reports_do_not_overwrite_cancelled_or_timed_out_targets() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new(
            "terminal-actions@example.com",
            "Terminal Actions",
            "hash",
        ))
        .await
        .unwrap();
    let org = store
        .create_org(
            Org::new("Terminal Actions Org", "terminal-actions"),
            user.id,
        )
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
    let cancel_action = store
        .create_action(Action::new(
            project.id,
            vec![device.id],
            "diagnostics.collect",
            json!({ "session_id": Uuid::now_v7() }),
            Some(user.id),
        ))
        .await
        .unwrap();
    store.claim_queued_action_targets(1).await.unwrap();
    store
        .transition_action_targets(ActionTargetTransition {
            project_id: project.id,
            action_id: cancel_action.id,
            device_ids: None,
            allowed_source_states: vec![ActionState::Running],
            next_state: ActionState::Cancelled,
            progress: None,
            errors: Some(vec!["operator cancelled".to_owned()]),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    let after_device_report = store
        .update_action_status(ActionStatusUpdate {
            project_id: project.id,
            action_id: cancel_action.id,
            device_id: device.id,
            state: ActionState::Completed,
            progress: 100,
            errors: Vec::new(),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(after_device_report.state, ActionState::Cancelled);
    assert_eq!(after_device_report.errors, vec!["operator cancelled"]);

    let timeout_action = store
        .create_action(Action::new(
            project.id,
            vec![device.id],
            "diagnostics.collect",
            json!({ "session_id": Uuid::now_v7() }),
            Some(user.id),
        ))
        .await
        .unwrap();
    store.claim_queued_action_targets(1).await.unwrap();
    let timed_out = store
        .timeout_running_action_targets(Utc::now() + chrono::Duration::seconds(1), 10, Utc::now())
        .await
        .unwrap();
    assert!(
        timed_out
            .iter()
            .any(|target| target.action_id == timeout_action.id)
    );
    let after_late_report = store
        .update_action_status(ActionStatusUpdate {
            project_id: project.id,
            action_id: timeout_action.id,
            device_id: device.id,
            state: ActionState::Completed,
            progress: 100,
            errors: Vec::new(),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(after_late_report.state, ActionState::TimedOut);
    assert_eq!(after_late_report.progress, 0);
}

#[tokio::test]
async fn action_target_transitions_cover_approval_retry_cancel_and_timeout() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new(
            "transition-actions@example.com",
            "Transition Actions",
            "hash",
        ))
        .await
        .unwrap();
    let org = store
        .create_org(
            Org::new("Transition Actions Org", "transition-actions"),
            user.id,
        )
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
    let mut action = Action::new(
        project.id,
        vec![device.id],
        "ota.install",
        json!({
            "firmware_id": Uuid::now_v7(),
            "component": "main",
            "version": "1.0.0",
            "signed_url": "https://objects.example/firmware/main.bin",
            "sha256": "a".repeat(64),
            "size_bytes": 1024
        }),
        Some(user.id),
    );
    action.state = ActionState::WaitingApproval;
    let action = store.create_action(action).await.unwrap();

    assert!(
        store
            .claim_queued_action_targets(10)
            .await
            .unwrap()
            .is_empty()
    );

    let approved = store
        .transition_action_targets(ActionTargetTransition {
            project_id: project.id,
            action_id: action.id,
            device_ids: None,
            allowed_source_states: vec![ActionState::WaitingApproval],
            next_state: ActionState::Queued,
            progress: Some(0),
            errors: Some(Vec::new()),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(approved.state, ActionState::Queued);

    assert_eq!(
        store.claim_queued_action_targets(10).await.unwrap().len(),
        1
    );
    let timed_out = store
        .timeout_running_action_targets(Utc::now() + chrono::Duration::seconds(1), 10, Utc::now())
        .await
        .unwrap();
    assert_eq!(timed_out.len(), 1);
    assert_eq!(timed_out[0].state, ActionState::TimedOut);

    let retried = store
        .transition_action_targets(ActionTargetTransition {
            project_id: project.id,
            action_id: action.id,
            device_ids: Some(vec![device.id]),
            allowed_source_states: vec![
                ActionState::Failed,
                ActionState::TimedOut,
                ActionState::Cancelled,
            ],
            next_state: ActionState::Queued,
            progress: Some(0),
            errors: Some(Vec::new()),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(retried.state, ActionState::Queued);

    let cancelled = store
        .transition_action_targets(ActionTargetTransition {
            project_id: project.id,
            action_id: action.id,
            device_ids: None,
            allowed_source_states: vec![
                ActionState::Queued,
                ActionState::WaitingApproval,
                ActionState::Running,
            ],
            next_state: ActionState::Cancelled,
            progress: None,
            errors: Some(vec!["operator cancelled".to_owned()]),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(cancelled.state, ActionState::Cancelled);
    assert_eq!(cancelled.errors, vec!["operator cancelled"]);
}

#[tokio::test]
async fn mirrors_unique_constraints_for_user_project_stream_firmware_certificate() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("unique@example.com", "Unique", "hash"))
        .await
        .unwrap();
    assert_eq!(
        store
            .create_user(User::new("UNIQUE@example.com", "Duplicate", "hash"))
            .await
            .unwrap_err(),
        StoreError::Conflict("user")
    );
    let org = store
        .create_org(Org::new("Unique Org", "unique"), user.id)
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

    assert_eq!(
        store
            .create_project(Project::new(org.id, "Factory Duplicate", "factory"))
            .await
            .unwrap_err(),
        StoreError::Conflict("project")
    );

    store
        .create_stream(StreamDefinition::new(project.id, "telemetry", Vec::new()))
        .await
        .unwrap();
    assert_eq!(
        store
            .create_stream(StreamDefinition::new(project.id, "telemetry", Vec::new()))
            .await
            .unwrap_err(),
        StoreError::Conflict("stream")
    );

    store
        .create_firmware(FirmwareArtifact::new(
            project.id,
            "main",
            "1.0.0",
            "firmware/main/1.0.0.bin",
            "sha256",
            "application/octet-stream",
            Some("ed25519:test".to_owned()),
            1024,
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .create_firmware(FirmwareArtifact::new(
                project.id,
                "main",
                "1.0.0",
                "firmware/main/1.0.0-copy.bin",
                "sha256",
                "application/octet-stream",
                None,
                1024,
            ))
            .await
            .unwrap_err(),
        StoreError::Conflict("firmware")
    );

    store
        .create_device_certificate(DeviceCertificate::new(
            project.id,
            device.id,
            "fingerprint",
            Utc::now(),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .create_device_certificate(DeviceCertificate::new(
                project.id,
                device.id,
                "fingerprint",
                Utc::now(),
            ))
            .await
            .unwrap_err(),
        StoreError::Conflict("certificate")
    );
}

#[tokio::test]
async fn firmware_finalize_and_rollout_are_project_scoped() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new(
            "firmware-flow@example.com",
            "Firmware Flow",
            "hash",
        ))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Firmware Flow Org", "firmware-flow"), user.id)
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
    let firmware = store
        .create_firmware(FirmwareArtifact::new(
            project.id,
            "main",
            "1.0.0",
            "projects/factory/firmware/main.bin",
            "a".repeat(64),
            "application/octet-stream",
            Some("ed25519:test".to_owned()),
            1024,
        ))
        .await
        .unwrap();

    assert_eq!(
        store
            .finalize_firmware(
                project.id,
                firmware.id,
                &"b".repeat(64),
                1024,
                Some("ed25519:test"),
                Utc::now()
            )
            .await
            .unwrap_err(),
        StoreError::Conflict("firmware verification")
    );
    let finalized = store
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
    assert!(finalized.verified_at.is_some());

    let action = store
        .create_action(Action::new(
            project.id,
            vec![device.id],
            "ota.install",
            json!({ "firmware_id": firmware.id }),
            Some(user.id),
        ))
        .await
        .unwrap();
    let rollout = store
        .create_firmware_rollout(FirmwareRollout::new(NewFirmwareRollout {
            project_id: project.id,
            firmware_id: firmware.id,
            action_id: action.id,
            cohort_size: 1,
            strategy: "cohort".to_owned(),
            rollback_strategy: Some("previous_version".to_owned()),
            state: FirmwareRolloutState::Running,
            created_by: Some(user.id),
        }))
        .await
        .unwrap();
    assert_eq!(rollout.action_id, action.id);
    assert_eq!(
        store.list_firmware_rollouts(project.id).await,
        vec![rollout]
    );
}

#[tokio::test]
async fn alert_events_dedupe_resolve_and_track_notification_attempts() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new(
            "alert-events@example.com",
            "Alert Events",
            "hash",
        ))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Alert Events Org", "alert-events"), user.id)
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
            id: Uuid::now_v7(),
            project_id: project.id,
            name: "offline".to_owned(),
            kind: AlertKind::Offline,
            expression: json!({}),
            enabled: true,
        })
        .await
        .unwrap();
    let event = AlertEvent::firing(NewAlertEvent {
        project_id: project.id,
        alert_rule_id: rule.id,
        device_id: Some(device.id),
        dedupe_key: "offline:press-1".to_owned(),
        message: "press-1 offline".to_owned(),
        observed_value: Some(360.0),
        threshold: Some(300.0),
        ts: Utc::now(),
    });
    let first = store
        .upsert_firing_alert_event(event.clone())
        .await
        .unwrap();
    let second = store.upsert_firing_alert_event(event).await.unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(
        store
            .record_alert_notification_attempt(
                project.id,
                first.id,
                Some("webhook failed".to_owned()),
                Utc::now()
            )
            .await
            .unwrap()
            .notification_attempts,
        1
    );
    let resolved = store
        .resolve_alert_event(project.id, rule.id, "offline:press-1", Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.state, AlertEventState::Resolved);
    assert_eq!(
        store
            .list_alert_events(project.id, Some(AlertEventState::Firing))
            .await
            .len(),
        0
    );
}

#[tokio::test]
async fn diagnostics_sessions_track_object_metadata() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("diagnostics@example.com", "Diagnostics", "hash"))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Diagnostics Org", "diagnostics"), user.id)
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
    let mut session = DiagnosticsSession::new(
        project.id,
        device.id,
        None,
        "projects/factory/diagnostics/session.tar.zst",
        Some(user.id),
    );
    session.state = DiagnosticsSessionState::UploadPending;
    let session = store.create_diagnostics_session(session).await.unwrap();
    let mut uploaded = session.clone();
    uploaded.state = DiagnosticsSessionState::Uploaded;
    uploaded.size_bytes = Some(2048);
    uploaded.sha256 = Some("c".repeat(64));
    uploaded.updated_at = Utc::now();

    let stored = store.update_diagnostics_session(uploaded).await.unwrap();
    assert_eq!(stored.state, DiagnosticsSessionState::Uploaded);
    assert_eq!(
        store
            .get_diagnostics_session(project.id, session.id)
            .await
            .unwrap()
            .sha256,
        Some("c".repeat(64))
    );
}

#[tokio::test]
async fn audit_requires_project_to_belong_to_org() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("audit@example.com", "Audit", "hash"))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("Audit Org", "audit"), user.id)
        .await
        .unwrap();
    let other_org = store
        .create_org(Org::new("Other Audit Org", "other-audit"), user.id)
        .await
        .unwrap();
    let other_project = store
        .create_project(Project::new(other_org.id, "Other Project", "other"))
        .await
        .unwrap();

    let error = store
        .append_audit(AuditLog::new(
            org.id,
            Some(other_project.id),
            Some(user.id),
            "audit.invalid",
            format!("project:{}", other_project.id),
            json!({}),
        ))
        .await
        .unwrap_err();

    assert_eq!(error, StoreError::TenantScope);
}

#[test]
fn database_error_display_is_opaque() {
    let error = StoreError::Database("relation users does not exist".to_owned());

    assert_eq!(error.to_string(), "database operation failed");
    assert!(format!("{error:?}").contains("relation users does not exist"));
}

#[tokio::test]
async fn sessions_rotate_refresh_tokens_and_detect_reuse() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("sessions@example.com", "Sessions", "hash"))
        .await
        .unwrap();
    let session = store
        .create_session(UserSession::new(
            user.id,
            "access-1",
            "refresh-1",
            Utc::now() + chrono::Duration::hours(1),
            Utc::now() + chrono::Duration::days(30),
        ))
        .await
        .unwrap();

    assert_eq!(
        store
            .get_active_session_by_token_hash("access-1")
            .await
            .unwrap()
            .user_id,
        user.id
    );

    let rotated = store
        .rotate_session_refresh_token(
            "refresh-1",
            "access-2".to_owned(),
            "refresh-2".to_owned(),
            Utc::now() + chrono::Duration::hours(1),
            Utc::now() + chrono::Duration::days(30),
        )
        .await
        .unwrap();
    assert_eq!(rotated.id, session.id);
    assert_eq!(
        store
            .get_active_session_by_token_hash("access-1")
            .await
            .unwrap_err(),
        StoreError::NotFound("session")
    );
    assert_eq!(
        store
            .rotate_session_refresh_token(
                "refresh-1",
                "access-3".to_owned(),
                "refresh-3".to_owned(),
                Utc::now() + chrono::Duration::hours(1),
                Utc::now() + chrono::Duration::days(30),
            )
            .await
            .unwrap_err(),
        StoreError::Conflict("refresh token reuse")
    );
    assert_eq!(
        store
            .get_active_session_by_token_hash("access-2")
            .await
            .unwrap_err(),
        StoreError::NotFound("session")
    );
}

#[tokio::test]
async fn sessions_reject_expired_and_revoked_refresh_tokens() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new(
            "session-expiry@example.com",
            "Session Expiry",
            "hash",
        ))
        .await
        .unwrap();
    store
        .create_session(UserSession::new(
            user.id,
            "expired-access",
            "valid-refresh-for-expired-access",
            Utc::now() - chrono::Duration::seconds(1),
            Utc::now() + chrono::Duration::days(30),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_session_by_token_hash("expired-access")
            .await
            .unwrap_err(),
        StoreError::NotFound("session")
    );

    store
        .create_session(UserSession::new(
            user.id,
            "valid-access-for-expired-refresh",
            "expired-refresh",
            Utc::now() + chrono::Duration::hours(1),
            Utc::now() - chrono::Duration::seconds(1),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .rotate_session_refresh_token(
                "expired-refresh",
                "unused-access".to_owned(),
                "unused-refresh".to_owned(),
                Utc::now() + chrono::Duration::hours(1),
                Utc::now() + chrono::Duration::days(30),
            )
            .await
            .unwrap_err(),
        StoreError::NotFound("refresh token")
    );

    store
        .create_session(UserSession::new(
            user.id,
            "revoked-access",
            "revoked-refresh",
            Utc::now() + chrono::Duration::hours(1),
            Utc::now() + chrono::Duration::days(30),
        ))
        .await
        .unwrap();
    store
        .revoke_session_by_token_hash("revoked-access")
        .await
        .unwrap();
    assert_eq!(
        store
            .rotate_session_refresh_token(
                "revoked-refresh",
                "unused-revoked-access".to_owned(),
                "unused-revoked-refresh".to_owned(),
                Utc::now() + chrono::Duration::hours(1),
                Utc::now() + chrono::Duration::days(30),
            )
            .await
            .unwrap_err(),
        StoreError::NotFound("refresh token")
    );
}

#[tokio::test]
async fn api_keys_are_hashed_scoped_and_revocable() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("api-keys@example.com", "API Keys", "hash"))
        .await
        .unwrap();
    let org = store
        .create_org(Org::new("API Key Org", "api-key-org"), user.id)
        .await
        .unwrap();
    let project = store
        .create_project(Project::new(org.id, "Factory", "factory"))
        .await
        .unwrap();
    let api_key = store
        .create_api_key(ApiKey::new(
            org.id,
            Some(project.id),
            "ingest",
            "hashed-secret",
            vec!["ingest:write".to_owned()],
            None,
            Some(user.id),
        ))
        .await
        .unwrap();

    assert!(api_key.has_scope("ingest:write"));
    assert_eq!(
        store
            .get_active_api_key_by_hash("hashed-secret")
            .await
            .unwrap()
            .id,
        api_key.id
    );
    assert_eq!(store.list_api_keys(org.id, Some(project.id)).await.len(), 1);
    let other_org = store
        .create_org(Org::new("Other API Key Org", "other-api-key-org"), user.id)
        .await
        .unwrap();
    assert_eq!(
        store
            .revoke_api_key(other_org.id, api_key.id)
            .await
            .unwrap_err(),
        StoreError::NotFound("api key")
    );
    store.revoke_api_key(org.id, api_key.id).await.unwrap();
    assert_eq!(
        store
            .get_active_api_key_by_hash("hashed-secret")
            .await
            .unwrap_err(),
        StoreError::NotFound("api key")
    );
    let expired = store
        .create_api_key(ApiKey::new(
            org.id,
            Some(project.id),
            "expired",
            "expired-secret",
            vec!["ingest:write".to_owned()],
            Some(Utc::now() - chrono::Duration::seconds(1)),
            Some(user.id),
        ))
        .await
        .unwrap();
    assert_eq!(expired.key_hash, "expired-secret");
    assert_eq!(
        store
            .get_active_api_key_by_hash("expired-secret")
            .await
            .unwrap_err(),
        StoreError::NotFound("api key")
    );
}

#[tokio::test]
async fn active_certificate_fingerprint_resolves_device_identity() {
    let store = MemoryStore::new();
    let user = store
        .create_user(User::new("certs@example.com", "Certs", "hash"))
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
    let certificate = store
        .create_device_certificate(DeviceCertificate::new(
            project.id,
            device.id,
            "fingerprint",
            Utc::now() + chrono::Duration::days(1),
        ))
        .await
        .unwrap();

    assert_eq!(
        store
            .get_active_device_by_certificate_fingerprint("fingerprint")
            .await
            .unwrap()
            .id,
        device.id
    );
    store
        .revoke_device_certificate(project.id, device.id, certificate.id)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_device_by_certificate_fingerprint("fingerprint")
            .await
            .unwrap_err(),
        StoreError::NotFound("certificate")
    );

    let mut future = DeviceCertificate::new(
        project.id,
        device.id,
        "future-fingerprint",
        Utc::now() + chrono::Duration::days(2),
    );
    future.not_before = Utc::now() + chrono::Duration::days(1);
    store.create_device_certificate(future).await.unwrap();
    assert_eq!(
        store
            .get_active_device_by_certificate_fingerprint("future-fingerprint")
            .await
            .unwrap_err(),
        StoreError::NotFound("certificate")
    );

    store
        .create_device_certificate(DeviceCertificate::new(
            project.id,
            device.id,
            "expired-fingerprint",
            Utc::now() - chrono::Duration::seconds(1),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_device_by_certificate_fingerprint("expired-fingerprint")
            .await
            .unwrap_err(),
        StoreError::NotFound("certificate")
    );

    let mut disabled_device = Device::new(project.id, "disabled-press", json!({}));
    disabled_device.status = DeviceStatus::Disabled;
    let disabled_device = store.create_device(disabled_device).await.unwrap();
    store
        .create_device_certificate(DeviceCertificate::new(
            project.id,
            disabled_device.id,
            "disabled-fingerprint",
            Utc::now() + chrono::Duration::days(1),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_device_by_certificate_fingerprint("disabled-fingerprint")
            .await
            .unwrap_err(),
        StoreError::NotFound("certificate")
    );
}

#[tokio::test]
async fn pg_store_contract_runs_when_database_url_is_set() {
    let Ok(database_url) = std::env::var("EXCALIBUR_SQL_TEST_DATABASE_URL") else {
        eprintln!("skipping PgStore contract; EXCALIBUR_SQL_TEST_DATABASE_URL is not set");
        return;
    };

    let pg_store = PgStore::connect(&database_url).await.unwrap();
    pg_store.validate_schema().await.unwrap();
    let store = Store::postgres(pg_store);
    let suffix = Uuid::now_v7().simple().to_string();
    let owner = store
        .create_user(User::new(
            format!("owner-{suffix}@example.com"),
            "SQL Owner",
            "hash",
        ))
        .await
        .unwrap();
    let viewer = store
        .create_user(User::new(
            format!("viewer-{suffix}@example.com"),
            "SQL Viewer",
            "hash",
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .create_user(User::new(
                format!("OWNER-{suffix}@example.com"),
                "Duplicate SQL Owner",
                "hash",
            ))
            .await
            .unwrap_err(),
        StoreError::Conflict("user")
    );
    let session = store
        .create_session(UserSession::new(
            owner.id,
            format!("sql-access-{suffix}-1"),
            format!("sql-refresh-{suffix}-1"),
            Utc::now() + chrono::Duration::hours(1),
            Utc::now() + chrono::Duration::days(30),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_session_by_token_hash(&format!("sql-access-{suffix}-1"))
            .await
            .unwrap()
            .id,
        session.id
    );
    store
        .rotate_session_refresh_token(
            &format!("sql-refresh-{suffix}-1"),
            format!("sql-access-{suffix}-2"),
            format!("sql-refresh-{suffix}-2"),
            Utc::now() + chrono::Duration::hours(1),
            Utc::now() + chrono::Duration::days(30),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .rotate_session_refresh_token(
                &format!("sql-refresh-{suffix}-1"),
                format!("sql-access-{suffix}-3"),
                format!("sql-refresh-{suffix}-3"),
                Utc::now() + chrono::Duration::hours(1),
                Utc::now() + chrono::Duration::days(30),
            )
            .await
            .unwrap_err(),
        StoreError::Conflict("refresh token reuse")
    );
    assert_eq!(
        store
            .get_active_session_by_token_hash(&format!("sql-access-{suffix}-2"))
            .await
            .unwrap_err(),
        StoreError::NotFound("session")
    );
    store
        .create_session(UserSession::new(
            owner.id,
            format!("sql-expired-access-{suffix}"),
            format!("sql-valid-refresh-for-expired-access-{suffix}"),
            Utc::now() - chrono::Duration::seconds(1),
            Utc::now() + chrono::Duration::days(30),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_session_by_token_hash(&format!("sql-expired-access-{suffix}"))
            .await
            .unwrap_err(),
        StoreError::NotFound("session")
    );
    store
        .create_session(UserSession::new(
            owner.id,
            format!("sql-valid-access-for-expired-refresh-{suffix}"),
            format!("sql-expired-refresh-{suffix}"),
            Utc::now() + chrono::Duration::hours(1),
            Utc::now() - chrono::Duration::seconds(1),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .rotate_session_refresh_token(
                &format!("sql-expired-refresh-{suffix}"),
                format!("sql-unused-access-{suffix}"),
                format!("sql-unused-refresh-{suffix}"),
                Utc::now() + chrono::Duration::hours(1),
                Utc::now() + chrono::Duration::days(30),
            )
            .await
            .unwrap_err(),
        StoreError::NotFound("refresh token")
    );
    store
        .create_session(UserSession::new(
            owner.id,
            format!("sql-revoked-access-{suffix}"),
            format!("sql-revoked-refresh-{suffix}"),
            Utc::now() + chrono::Duration::hours(1),
            Utc::now() + chrono::Duration::days(30),
        ))
        .await
        .unwrap();
    store
        .revoke_session_by_token_hash(&format!("sql-revoked-access-{suffix}"))
        .await
        .unwrap();
    assert_eq!(
        store
            .rotate_session_refresh_token(
                &format!("sql-revoked-refresh-{suffix}"),
                format!("sql-unused-revoked-access-{suffix}"),
                format!("sql-unused-revoked-refresh-{suffix}"),
                Utc::now() + chrono::Duration::hours(1),
                Utc::now() + chrono::Duration::days(30),
            )
            .await
            .unwrap_err(),
        StoreError::NotFound("refresh token")
    );
    let org = store
        .create_org(
            Org::new("SQL Contract Org", format!("sql-contract-{suffix}")),
            owner.id,
        )
        .await
        .unwrap();
    store
        .add_membership(Membership::new(org.id, viewer.id, Role::Viewer))
        .await
        .unwrap();
    let project = store
        .create_project(Project::new(
            org.id,
            "SQL Contract Project",
            format!("sql-contract-{suffix}"),
        ))
        .await
        .unwrap();
    let api_key = store
        .create_api_key(ApiKey::new(
            org.id,
            Some(project.id),
            "SQL ingest",
            format!("sql-api-key-{suffix}"),
            vec!["ingest:write".to_owned()],
            None,
            Some(owner.id),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_api_key_by_hash(&format!("sql-api-key-{suffix}"))
            .await
            .unwrap()
            .id,
        api_key.id
    );
    assert_eq!(
        store
            .list_api_keys(org.id, Some(project.id))
            .await
            .unwrap()
            .len(),
        1
    );
    store
        .create_api_key(ApiKey::new(
            org.id,
            Some(project.id),
            "Expired SQL ingest",
            format!("sql-expired-api-key-{suffix}"),
            vec!["ingest:write".to_owned()],
            Some(Utc::now() - chrono::Duration::seconds(1)),
            Some(owner.id),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_api_key_by_hash(&format!("sql-expired-api-key-{suffix}"))
            .await
            .unwrap_err(),
        StoreError::NotFound("api key")
    );
    let other_org = store
        .create_org(
            Org::new(
                "Other SQL Contract Org",
                format!("other-sql-contract-{suffix}"),
            ),
            owner.id,
        )
        .await
        .unwrap();
    let other_project = store
        .create_project(Project::new(
            other_org.id,
            "Other SQL Contract Project",
            format!("other-sql-contract-{suffix}"),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .revoke_api_key(other_org.id, api_key.id)
            .await
            .unwrap_err(),
        StoreError::NotFound("api key")
    );
    assert_eq!(
        store.user_role(org.id, owner.id).await.unwrap(),
        Some(Role::Owner)
    );
    assert_eq!(
        store
            .get_project_for_user(project.id, viewer.id)
            .await
            .unwrap()
            .id,
        project.id
    );
    assert_eq!(
        store
            .get_project_for_user(other_project.id, viewer.id)
            .await
            .unwrap_err(),
        StoreError::TenantScope
    );

    let device = store
        .create_device(Device::new(
            project.id,
            "sql-device",
            json!({"site": "lab"}),
        ))
        .await
        .unwrap();
    let other_device = store
        .create_device(Device::new(other_project.id, "other-sql-device", json!({})))
        .await
        .unwrap();
    let second_device = store
        .create_device(Device::new(project.id, "second-sql-device", json!({})))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_device(other_project.id, device.id)
            .await
            .unwrap_err(),
        StoreError::NotFound("device")
    );
    let certificate = store
        .create_device_certificate(DeviceCertificate::new(
            project.id,
            device.id,
            format!("fingerprint-{suffix}"),
            Utc::now() + chrono::Duration::days(1),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_device_by_certificate_fingerprint(&format!("fingerprint-{suffix}"))
            .await
            .unwrap()
            .id,
        device.id
    );
    assert_eq!(
        store
            .list_device_certificates(project.id, device.id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .revoke_device_certificate(project.id, device.id, certificate.id)
            .await
            .unwrap()
            .status,
        CertificateStatus::Revoked
    );
    assert_eq!(
        store
            .revoke_device_certificate(other_project.id, other_device.id, certificate.id)
            .await
            .unwrap_err(),
        StoreError::TenantScope
    );
    let mut future_certificate = DeviceCertificate::new(
        project.id,
        device.id,
        format!("future-fingerprint-{suffix}"),
        Utc::now() + chrono::Duration::days(2),
    );
    future_certificate.not_before = Utc::now() + chrono::Duration::days(1);
    store
        .create_device_certificate(future_certificate)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_device_by_certificate_fingerprint(&format!("future-fingerprint-{suffix}"))
            .await
            .unwrap_err(),
        StoreError::NotFound("certificate")
    );
    store
        .create_device_certificate(DeviceCertificate::new(
            project.id,
            device.id,
            format!("expired-fingerprint-{suffix}"),
            Utc::now() - chrono::Duration::seconds(1),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_device_by_certificate_fingerprint(&format!("expired-fingerprint-{suffix}"))
            .await
            .unwrap_err(),
        StoreError::NotFound("certificate")
    );
    let mut disabled_device = Device::new(project.id, "disabled-sql-device", json!({}));
    disabled_device.status = DeviceStatus::Disabled;
    let disabled_device = store.create_device(disabled_device).await.unwrap();
    store
        .create_device_certificate(DeviceCertificate::new(
            project.id,
            disabled_device.id,
            format!("disabled-fingerprint-{suffix}"),
            Utc::now() + chrono::Duration::days(1),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_active_device_by_certificate_fingerprint(&format!("disabled-fingerprint-{suffix}"))
            .await
            .unwrap_err(),
        StoreError::NotFound("certificate")
    );

    let stream = store
        .create_stream(StreamDefinition::new(
            project.id,
            format!("temperature-{suffix}"),
            vec![StreamField {
                name: "value".to_owned(),
                field_type: StreamFieldType::Float,
                required: true,
            }],
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .create_stream(StreamDefinition::new(
                project.id,
                stream.name.clone(),
                Vec::new()
            ))
            .await
            .unwrap_err(),
        StoreError::Conflict("stream")
    );
    store
        .write_telemetry(vec![TelemetryPoint {
            project_id: project.id,
            device_id: device.id,
            stream: stream.name.clone(),
            sequence: 1,
            ts: Utc::now(),
            payload: json!({"value": 21.5}),
            ingested_at: Utc::now(),
        }])
        .await
        .unwrap();
    let telemetry = store
        .query_telemetry(project.id, Some(device.id), Some(&stream.name), 10)
        .await
        .unwrap();
    assert_eq!(telemetry.len(), 1);
    assert_eq!(telemetry[0].payload["value"], 21.5);
    assert_eq!(
        store
            .write_telemetry(vec![TelemetryPoint {
                project_id: project.id,
                device_id: device.id,
                stream: stream.name.clone(),
                sequence: 1,
                ts: Utc::now() + chrono::Duration::seconds(1),
                payload: json!({"value": 22.0}),
                ingested_at: Utc::now(),
            }])
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .query_telemetry(project.id, Some(device.id), Some(&stream.name), 10)
            .await
            .unwrap()
            .len(),
        1
    );
    let rollback_stream = format!("rollback-{suffix}");
    assert_eq!(
        store
            .write_telemetry(vec![
                TelemetryPoint {
                    project_id: project.id,
                    device_id: device.id,
                    stream: rollback_stream.clone(),
                    sequence: 1,
                    ts: Utc::now(),
                    payload: json!({"value": 1}),
                    ingested_at: Utc::now(),
                },
                TelemetryPoint {
                    project_id: project.id,
                    device_id: other_device.id,
                    stream: rollback_stream.clone(),
                    sequence: 2,
                    ts: Utc::now(),
                    payload: json!({"value": 2}),
                    ingested_at: Utc::now(),
                },
            ])
            .await
            .unwrap_err(),
        StoreError::TenantScope
    );
    assert!(
        store
            .query_telemetry(project.id, None, Some(&rollback_stream), 10)
            .await
            .unwrap()
            .is_empty()
    );

    let action = store
        .create_action(Action::new(
            project.id,
            vec![device.id],
            "diagnostics.collect",
            json!({"session_id": Uuid::now_v7()}),
            Some(owner.id),
        ))
        .await
        .unwrap();
    let claimed = store.claim_queued_action_targets(1).await.unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].action_id, action.id);
    assert_eq!(
        store
            .get_action_target_state(project.id, action.id, device.id)
            .await
            .unwrap(),
        ActionState::Running
    );
    let action = store
        .update_action_status(ActionStatusUpdate {
            project_id: project.id,
            action_id: action.id,
            device_id: device.id,
            state: ActionState::Completed,
            progress: 100,
            errors: Vec::new(),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(action.state, ActionState::Completed);
    let batch_action = store
        .create_action(Action::new(
            project.id,
            vec![device.id, second_device.id],
            "diagnostics.collect",
            json!({"session_id": Uuid::now_v7()}),
            Some(owner.id),
        ))
        .await
        .unwrap();
    let claimed = store.claim_queued_action_targets(2).await.unwrap();
    assert_eq!(claimed.len(), 2);
    assert!(
        claimed
            .iter()
            .all(|target| target.action_id == batch_action.id)
    );
    let partial_batch_action = store
        .update_action_status(ActionStatusUpdate {
            project_id: project.id,
            action_id: batch_action.id,
            device_id: device.id,
            state: ActionState::Completed,
            progress: 100,
            errors: Vec::new(),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(partial_batch_action.state, ActionState::Running);
    assert_eq!(partial_batch_action.progress, 50);
    let completed_batch_action = store
        .update_action_status(ActionStatusUpdate {
            project_id: project.id,
            action_id: batch_action.id,
            device_id: second_device.id,
            state: ActionState::Completed,
            progress: 100,
            errors: Vec::new(),
            ts: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(completed_batch_action.state, ActionState::Completed);
    assert_eq!(completed_batch_action.progress, 100);
    let scoped_action = store
        .create_action(Action::new(
            project.id,
            vec![device.id],
            "diagnostics.collect",
            json!({"session_id": Uuid::now_v7()}),
            Some(owner.id),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .update_action_status(ActionStatusUpdate {
                project_id: project.id,
                action_id: scoped_action.id,
                device_id: other_device.id,
                state: ActionState::Completed,
                progress: 100,
                errors: Vec::new(),
                ts: Utc::now(),
            })
            .await
            .unwrap_err(),
        StoreError::TenantScope
    );
    let scoped_action = store
        .list_actions(project.id)
        .await
        .unwrap()
        .into_iter()
        .find(|action| action.id == scoped_action.id)
        .unwrap();
    assert_eq!(scoped_action.state, ActionState::Queued);

    let mut approval_action = Action::new(
        project.id,
        vec![device.id],
        "diagnostics.collect",
        json!({"session_id": Uuid::now_v7()}),
        Some(owner.id),
    );
    approval_action.state = ActionState::WaitingApproval;
    let approval_action = store.create_action(approval_action).await.unwrap();
    assert!(
        store
            .claim_queued_action_targets(10)
            .await
            .unwrap()
            .iter()
            .all(|target| target.action_id != approval_action.id)
    );
    assert_eq!(
        store
            .transition_action_targets(ActionTargetTransition {
                project_id: project.id,
                action_id: approval_action.id,
                device_ids: None,
                allowed_source_states: vec![ActionState::WaitingApproval],
                next_state: ActionState::Queued,
                progress: Some(0),
                errors: Some(Vec::new()),
                ts: Utc::now(),
            })
            .await
            .unwrap()
            .state,
        ActionState::Queued
    );
    assert!(
        store
            .claim_queued_action_targets(10)
            .await
            .unwrap()
            .iter()
            .any(|target| target.action_id == approval_action.id)
    );
    assert_eq!(
        store
            .timeout_running_action_targets(
                Utc::now() + chrono::Duration::seconds(1),
                10,
                Utc::now(),
            )
            .await
            .unwrap()
            .iter()
            .filter(|target| target.action_id == approval_action.id)
            .count(),
        1
    );
    assert_eq!(
        store
            .transition_action_targets(ActionTargetTransition {
                project_id: project.id,
                action_id: approval_action.id,
                device_ids: Some(vec![device.id]),
                allowed_source_states: vec![
                    ActionState::Failed,
                    ActionState::TimedOut,
                    ActionState::Cancelled,
                ],
                next_state: ActionState::Queued,
                progress: Some(0),
                errors: Some(Vec::new()),
                ts: Utc::now(),
            })
            .await
            .unwrap()
            .state,
        ActionState::Queued
    );
    assert_eq!(
        store
            .transition_action_targets(ActionTargetTransition {
                project_id: project.id,
                action_id: approval_action.id,
                device_ids: None,
                allowed_source_states: vec![
                    ActionState::Queued,
                    ActionState::WaitingApproval,
                    ActionState::Running,
                ],
                next_state: ActionState::Cancelled,
                progress: None,
                errors: Some(vec!["operator cancelled".to_owned()]),
                ts: Utc::now(),
            })
            .await
            .unwrap()
            .state,
        ActionState::Cancelled
    );
    assert_eq!(
        store
            .get_action_target_state(project.id, approval_action.id, device.id)
            .await
            .unwrap(),
        ActionState::Cancelled
    );

    store
        .create_firmware(FirmwareArtifact::new(
            project.id,
            "main",
            format!("1.0.0-{suffix}"),
            format!("projects/{}/firmware/{suffix}.bin", project.id),
            "a".repeat(64),
            "application/octet-stream",
            Some("ed25519:test".to_owned()),
            1024,
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .create_firmware(FirmwareArtifact::new(
                project.id,
                "main",
                format!("1.0.0-{suffix}"),
                format!("projects/{}/firmware/{suffix}-copy.bin", project.id),
                "a".repeat(64),
                "application/octet-stream",
                None,
                1024,
            ))
            .await
            .unwrap_err(),
        StoreError::Conflict("firmware")
    );
    store
        .create_dashboard(Dashboard {
            id: Uuid::now_v7(),
            project_id: project.id,
            name: "SQL Dashboard".to_owned(),
            layout: json!({"columns": 2}),
        })
        .await
        .unwrap();
    store
        .create_alert(AlertRule {
            id: Uuid::now_v7(),
            project_id: project.id,
            name: "SQL Alert".to_owned(),
            kind: AlertKind::Threshold,
            expression: json!({"field": "value", "gt": 80}),
            enabled: true,
        })
        .await
        .unwrap();
    store
        .append_audit(AuditLog::new(
            org.id,
            Some(project.id),
            Some(owner.id),
            "sql.contract",
            format!("project:{}", project.id),
            json!({"ok": true}),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .append_audit(AuditLog::new(
                org.id,
                Some(other_project.id),
                Some(owner.id),
                "sql.invalid_audit_scope",
                format!("project:{}", other_project.id),
                json!({"ok": false}),
            ))
            .await
            .unwrap_err(),
        StoreError::TenantScope
    );

    assert!(!store.list_firmware(project.id).await.unwrap().is_empty());
    assert!(!store.list_dashboards(project.id).await.unwrap().is_empty());
    assert!(!store.list_alerts(project.id).await.unwrap().is_empty());
    assert!(
        !store
            .list_audit(org.id, Some(project.id))
            .await
            .unwrap()
            .is_empty()
    );
}
