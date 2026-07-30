use chrono::Utc;
use excalibur_domain::{
    Action, ActionState, ActionStatusUpdate, AlertKind, AlertRule, ApiKey, AuditLog,
    CertificateStatus, Dashboard, Device, DeviceCertificate, DeviceStatus, FirmwareArtifact,
    Membership, Org, Project, Role, StreamDefinition, StreamField, StreamFieldType, TelemetryPoint,
    User, UserSession,
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

    store
        .create_firmware(FirmwareArtifact::new(
            project.id,
            "main",
            format!("1.0.0-{suffix}"),
            format!("firmware/{suffix}.bin"),
            "a".repeat(64),
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
                format!("firmware/{suffix}-copy.bin"),
                "a".repeat(64),
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
