use chrono::{DateTime, Utc};
use excalibur_domain::{
    Action, ActionDispatchTarget, ActionStatusUpdate, ActionTargetStatusChange,
    ActionTargetTransition, AlertEvent, AlertEventState, AlertRule, ApiKey, AuditLog, Dashboard,
    Device, DeviceCertificate, DiagnosticsSession, FirmwareArtifact, FirmwareRollout, Id,
    Membership, Org, Project, Role, StreamDefinition, TelemetryAggregateBucket, TelemetryPoint,
    User, UserSession,
};
use serde_json::Value;

use crate::{MemoryStore, PgStore, StoreResult};

#[derive(Debug, Clone)]
pub enum Store {
    Memory(MemoryStore),
    Postgres(PgStore),
}

impl Default for Store {
    fn default() -> Self {
        Self::memory()
    }
}

impl Store {
    pub fn memory() -> Self {
        Self::Memory(MemoryStore::new())
    }

    pub fn postgres(store: PgStore) -> Self {
        Self::Postgres(store)
    }

    pub async fn health_check(&self) -> StoreResult<()> {
        match self {
            Store::Memory(_) => Ok(()),
            Store::Postgres(store) => store.health_check().await,
        }
    }

    pub async fn create_user(&self, user: User) -> StoreResult<User> {
        match self {
            Store::Memory(store) => store.create_user(user).await,
            Store::Postgres(store) => store.create_user(user).await,
        }
    }

    pub async fn get_user_by_email(&self, email: &str) -> StoreResult<User> {
        match self {
            Store::Memory(store) => store.get_user_by_email(email).await,
            Store::Postgres(store) => store.get_user_by_email(email).await,
        }
    }

    pub async fn create_session(&self, session: UserSession) -> StoreResult<UserSession> {
        match self {
            Store::Memory(store) => store.create_session(session).await,
            Store::Postgres(store) => store.create_session(session).await,
        }
    }

    pub async fn get_active_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> StoreResult<UserSession> {
        match self {
            Store::Memory(store) => store.get_active_session_by_token_hash(token_hash).await,
            Store::Postgres(store) => store.get_active_session_by_token_hash(token_hash).await,
        }
    }

    pub async fn rotate_session_refresh_token(
        &self,
        refresh_token_hash: &str,
        next_token_hash: String,
        next_refresh_token_hash: String,
        next_expires_at: chrono::DateTime<chrono::Utc>,
        next_refresh_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> StoreResult<UserSession> {
        match self {
            Store::Memory(store) => {
                store
                    .rotate_session_refresh_token(
                        refresh_token_hash,
                        next_token_hash,
                        next_refresh_token_hash,
                        next_expires_at,
                        next_refresh_expires_at,
                    )
                    .await
            }
            Store::Postgres(store) => {
                store
                    .rotate_session_refresh_token(
                        refresh_token_hash,
                        next_token_hash,
                        next_refresh_token_hash,
                        next_expires_at,
                        next_refresh_expires_at,
                    )
                    .await
            }
        }
    }

    pub async fn revoke_session_by_token_hash(&self, token_hash: &str) -> StoreResult<()> {
        match self {
            Store::Memory(store) => store.revoke_session_by_token_hash(token_hash).await,
            Store::Postgres(store) => store.revoke_session_by_token_hash(token_hash).await,
        }
    }

    pub async fn create_api_key(&self, api_key: ApiKey) -> StoreResult<ApiKey> {
        match self {
            Store::Memory(store) => store.create_api_key(api_key).await,
            Store::Postgres(store) => store.create_api_key(api_key).await,
        }
    }

    pub async fn get_active_api_key_by_hash(&self, key_hash: &str) -> StoreResult<ApiKey> {
        match self {
            Store::Memory(store) => store.get_active_api_key_by_hash(key_hash).await,
            Store::Postgres(store) => store.get_active_api_key_by_hash(key_hash).await,
        }
    }

    pub async fn list_api_keys(
        &self,
        org_id: Id,
        project_id: Option<Id>,
    ) -> StoreResult<Vec<ApiKey>> {
        match self {
            Store::Memory(store) => Ok(store.list_api_keys(org_id, project_id).await),
            Store::Postgres(store) => store.list_api_keys(org_id, project_id).await,
        }
    }

    pub async fn revoke_api_key(&self, org_id: Id, api_key_id: Id) -> StoreResult<ApiKey> {
        match self {
            Store::Memory(store) => store.revoke_api_key(org_id, api_key_id).await,
            Store::Postgres(store) => store.revoke_api_key(org_id, api_key_id).await,
        }
    }

    pub async fn create_org(&self, org: Org, owner_id: Id) -> StoreResult<Org> {
        match self {
            Store::Memory(store) => store.create_org(org, owner_id).await,
            Store::Postgres(store) => store.create_org(org, owner_id).await,
        }
    }

    pub async fn add_membership(&self, membership: Membership) -> StoreResult<Membership> {
        match self {
            Store::Memory(store) => store.add_membership(membership).await,
            Store::Postgres(store) => store.add_membership(membership).await,
        }
    }

    pub async fn list_orgs_for_user(&self, user_id: Id) -> StoreResult<Vec<Org>> {
        match self {
            Store::Memory(store) => Ok(store.list_orgs_for_user(user_id).await),
            Store::Postgres(store) => store.list_orgs_for_user(user_id).await,
        }
    }

    pub async fn user_role(&self, org_id: Id, user_id: Id) -> StoreResult<Option<Role>> {
        match self {
            Store::Memory(store) => Ok(store.user_role(org_id, user_id).await),
            Store::Postgres(store) => store.user_role(org_id, user_id).await,
        }
    }

    pub async fn create_project(&self, project: Project) -> StoreResult<Project> {
        match self {
            Store::Memory(store) => store.create_project(project).await,
            Store::Postgres(store) => store.create_project(project).await,
        }
    }

    pub async fn list_projects(&self, org_id: Id) -> StoreResult<Vec<Project>> {
        match self {
            Store::Memory(store) => Ok(store.list_projects(org_id).await),
            Store::Postgres(store) => store.list_projects(org_id).await,
        }
    }

    pub async fn get_project(&self, project_id: Id) -> StoreResult<Project> {
        match self {
            Store::Memory(store) => store.get_project(project_id).await,
            Store::Postgres(store) => store.get_project(project_id).await,
        }
    }

    pub async fn get_project_for_user(&self, project_id: Id, user_id: Id) -> StoreResult<Project> {
        match self {
            Store::Memory(store) => store.get_project_for_user(project_id, user_id).await,
            Store::Postgres(store) => store.get_project_for_user(project_id, user_id).await,
        }
    }

    pub async fn create_device(&self, device: Device) -> StoreResult<Device> {
        match self {
            Store::Memory(store) => store.create_device(device).await,
            Store::Postgres(store) => store.create_device(device).await,
        }
    }

    pub async fn list_devices(&self, project_id: Id) -> StoreResult<Vec<Device>> {
        match self {
            Store::Memory(store) => Ok(store.list_devices(project_id).await),
            Store::Postgres(store) => store.list_devices(project_id).await,
        }
    }

    pub async fn get_device(&self, project_id: Id, device_id: Id) -> StoreResult<Device> {
        match self {
            Store::Memory(store) => store.get_device(project_id, device_id).await,
            Store::Postgres(store) => store.get_device(project_id, device_id).await,
        }
    }

    pub async fn create_device_certificate(
        &self,
        certificate: DeviceCertificate,
    ) -> StoreResult<DeviceCertificate> {
        match self {
            Store::Memory(store) => store.create_device_certificate(certificate).await,
            Store::Postgres(store) => store.create_device_certificate(certificate).await,
        }
    }

    pub async fn list_device_certificates(
        &self,
        project_id: Id,
        device_id: Id,
    ) -> StoreResult<Vec<DeviceCertificate>> {
        match self {
            Store::Memory(store) => store.list_device_certificates(project_id, device_id).await,
            Store::Postgres(store) => store.list_device_certificates(project_id, device_id).await,
        }
    }

    pub async fn get_active_device_by_certificate_fingerprint(
        &self,
        fingerprint_sha256: &str,
    ) -> StoreResult<Device> {
        match self {
            Store::Memory(store) => {
                store
                    .get_active_device_by_certificate_fingerprint(fingerprint_sha256)
                    .await
            }
            Store::Postgres(store) => {
                store
                    .get_active_device_by_certificate_fingerprint(fingerprint_sha256)
                    .await
            }
        }
    }

    pub async fn revoke_device_certificate(
        &self,
        project_id: Id,
        device_id: Id,
        certificate_id: Id,
    ) -> StoreResult<DeviceCertificate> {
        match self {
            Store::Memory(store) => {
                store
                    .revoke_device_certificate(project_id, device_id, certificate_id)
                    .await
            }
            Store::Postgres(store) => {
                store
                    .revoke_device_certificate(project_id, device_id, certificate_id)
                    .await
            }
        }
    }

    pub async fn touch_device_online(&self, project_id: Id, device_id: Id) -> StoreResult<Device> {
        match self {
            Store::Memory(store) => store.touch_device_online(project_id, device_id).await,
            Store::Postgres(store) => store.touch_device_online(project_id, device_id).await,
        }
    }

    pub async fn update_shadow(
        &self,
        project_id: Id,
        device_id: Id,
        shadow: Value,
    ) -> StoreResult<Device> {
        match self {
            Store::Memory(store) => store.update_shadow(project_id, device_id, shadow).await,
            Store::Postgres(store) => store.update_shadow(project_id, device_id, shadow).await,
        }
    }

    pub async fn create_stream(&self, stream: StreamDefinition) -> StoreResult<StreamDefinition> {
        match self {
            Store::Memory(store) => store.create_stream(stream).await,
            Store::Postgres(store) => store.create_stream(stream).await,
        }
    }

    pub async fn list_streams(&self, project_id: Id) -> StoreResult<Vec<StreamDefinition>> {
        match self {
            Store::Memory(store) => Ok(store.list_streams(project_id).await),
            Store::Postgres(store) => store.list_streams(project_id).await,
        }
    }

    pub async fn write_telemetry(&self, points: Vec<TelemetryPoint>) -> StoreResult<usize> {
        match self {
            Store::Memory(store) => store.write_telemetry(points).await,
            Store::Postgres(store) => store.write_telemetry(points).await,
        }
    }

    pub async fn query_telemetry(
        &self,
        project_id: Id,
        device_id: Option<Id>,
        stream: Option<&str>,
        limit: usize,
    ) -> StoreResult<Vec<TelemetryPoint>> {
        match self {
            Store::Memory(store) => Ok(store
                .query_telemetry(project_id, device_id, stream, limit)
                .await),
            Store::Postgres(store) => {
                store
                    .query_telemetry(project_id, device_id, stream, limit)
                    .await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn aggregate_telemetry(
        &self,
        project_id: Id,
        device_id: Option<Id>,
        stream: &str,
        field: Option<&str>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        bucket_seconds: i64,
        limit: usize,
    ) -> StoreResult<Vec<TelemetryAggregateBucket>> {
        match self {
            Store::Memory(store) => Ok(store
                .aggregate_telemetry(
                    project_id,
                    device_id,
                    stream,
                    field,
                    from,
                    to,
                    bucket_seconds,
                    limit,
                )
                .await),
            Store::Postgres(store) => {
                store
                    .aggregate_telemetry(
                        project_id,
                        device_id,
                        stream,
                        field,
                        from,
                        to,
                        bucket_seconds,
                        limit,
                    )
                    .await
            }
        }
    }

    pub async fn create_action(&self, action: Action) -> StoreResult<Action> {
        match self {
            Store::Memory(store) => store.create_action(action).await,
            Store::Postgres(store) => store.create_action(action).await,
        }
    }

    pub async fn list_actions(&self, project_id: Id) -> StoreResult<Vec<Action>> {
        match self {
            Store::Memory(store) => Ok(store.list_actions(project_id).await),
            Store::Postgres(store) => store.list_actions(project_id).await,
        }
    }

    pub async fn update_action_status(&self, update: ActionStatusUpdate) -> StoreResult<Action> {
        match self {
            Store::Memory(store) => store.update_action_status(update).await,
            Store::Postgres(store) => store.update_action_status(update).await,
        }
    }

    pub async fn claim_queued_action_targets(
        &self,
        limit: usize,
    ) -> StoreResult<Vec<ActionDispatchTarget>> {
        match self {
            Store::Memory(store) => store.claim_queued_action_targets(limit).await,
            Store::Postgres(store) => store.claim_queued_action_targets(limit).await,
        }
    }

    pub async fn transition_action_targets(
        &self,
        transition: ActionTargetTransition,
    ) -> StoreResult<Action> {
        match self {
            Store::Memory(store) => store.transition_action_targets(transition).await,
            Store::Postgres(store) => store.transition_action_targets(transition).await,
        }
    }

    pub async fn timeout_running_action_targets(
        &self,
        older_than: DateTime<Utc>,
        limit: usize,
        ts: DateTime<Utc>,
    ) -> StoreResult<Vec<ActionTargetStatusChange>> {
        match self {
            Store::Memory(store) => {
                store
                    .timeout_running_action_targets(older_than, limit, ts)
                    .await
            }
            Store::Postgres(store) => {
                store
                    .timeout_running_action_targets(older_than, limit, ts)
                    .await
            }
        }
    }

    pub async fn create_firmware(
        &self,
        artifact: FirmwareArtifact,
    ) -> StoreResult<FirmwareArtifact> {
        match self {
            Store::Memory(store) => store.create_firmware(artifact).await,
            Store::Postgres(store) => store.create_firmware(artifact).await,
        }
    }

    pub async fn finalize_firmware(
        &self,
        project_id: Id,
        firmware_id: Id,
        sha256: &str,
        size_bytes: i64,
        signature: Option<&str>,
        ts: DateTime<Utc>,
    ) -> StoreResult<FirmwareArtifact> {
        match self {
            Store::Memory(store) => {
                store
                    .finalize_firmware(project_id, firmware_id, sha256, size_bytes, signature, ts)
                    .await
            }
            Store::Postgres(store) => {
                store
                    .finalize_firmware(project_id, firmware_id, sha256, size_bytes, signature, ts)
                    .await
            }
        }
    }

    pub async fn list_firmware(&self, project_id: Id) -> StoreResult<Vec<FirmwareArtifact>> {
        match self {
            Store::Memory(store) => Ok(store.list_firmware(project_id).await),
            Store::Postgres(store) => store.list_firmware(project_id).await,
        }
    }

    pub async fn create_firmware_rollout(
        &self,
        rollout: FirmwareRollout,
    ) -> StoreResult<FirmwareRollout> {
        match self {
            Store::Memory(store) => store.create_firmware_rollout(rollout).await,
            Store::Postgres(store) => store.create_firmware_rollout(rollout).await,
        }
    }

    pub async fn list_firmware_rollouts(
        &self,
        project_id: Id,
    ) -> StoreResult<Vec<FirmwareRollout>> {
        match self {
            Store::Memory(store) => Ok(store.list_firmware_rollouts(project_id).await),
            Store::Postgres(store) => store.list_firmware_rollouts(project_id).await,
        }
    }

    pub async fn create_alert(&self, alert: AlertRule) -> StoreResult<AlertRule> {
        match self {
            Store::Memory(store) => store.create_alert(alert).await,
            Store::Postgres(store) => store.create_alert(alert).await,
        }
    }

    pub async fn list_alerts(&self, project_id: Id) -> StoreResult<Vec<AlertRule>> {
        match self {
            Store::Memory(store) => Ok(store.list_alerts(project_id).await),
            Store::Postgres(store) => store.list_alerts(project_id).await,
        }
    }

    pub async fn list_enabled_alerts(&self) -> StoreResult<Vec<AlertRule>> {
        match self {
            Store::Memory(store) => Ok(store.list_enabled_alerts().await),
            Store::Postgres(store) => store.list_enabled_alerts().await,
        }
    }

    pub async fn upsert_firing_alert_event(&self, event: AlertEvent) -> StoreResult<AlertEvent> {
        match self {
            Store::Memory(store) => store.upsert_firing_alert_event(event).await,
            Store::Postgres(store) => store.upsert_firing_alert_event(event).await,
        }
    }

    pub async fn resolve_alert_event(
        &self,
        project_id: Id,
        alert_rule_id: Id,
        dedupe_key: &str,
        ts: DateTime<Utc>,
    ) -> StoreResult<Option<AlertEvent>> {
        match self {
            Store::Memory(store) => {
                store
                    .resolve_alert_event(project_id, alert_rule_id, dedupe_key, ts)
                    .await
            }
            Store::Postgres(store) => {
                store
                    .resolve_alert_event(project_id, alert_rule_id, dedupe_key, ts)
                    .await
            }
        }
    }

    pub async fn list_alert_events(
        &self,
        project_id: Id,
        state_filter: Option<AlertEventState>,
    ) -> StoreResult<Vec<AlertEvent>> {
        match self {
            Store::Memory(store) => Ok(store.list_alert_events(project_id, state_filter).await),
            Store::Postgres(store) => store.list_alert_events(project_id, state_filter).await,
        }
    }

    pub async fn record_alert_notification_attempt(
        &self,
        project_id: Id,
        alert_event_id: Id,
        error: Option<String>,
        ts: DateTime<Utc>,
    ) -> StoreResult<AlertEvent> {
        match self {
            Store::Memory(store) => {
                store
                    .record_alert_notification_attempt(project_id, alert_event_id, error, ts)
                    .await
            }
            Store::Postgres(store) => {
                store
                    .record_alert_notification_attempt(project_id, alert_event_id, error, ts)
                    .await
            }
        }
    }

    pub async fn create_dashboard(&self, dashboard: Dashboard) -> StoreResult<Dashboard> {
        match self {
            Store::Memory(store) => store.create_dashboard(dashboard).await,
            Store::Postgres(store) => store.create_dashboard(dashboard).await,
        }
    }

    pub async fn list_dashboards(&self, project_id: Id) -> StoreResult<Vec<Dashboard>> {
        match self {
            Store::Memory(store) => Ok(store.list_dashboards(project_id).await),
            Store::Postgres(store) => store.list_dashboards(project_id).await,
        }
    }

    pub async fn create_diagnostics_session(
        &self,
        session: DiagnosticsSession,
    ) -> StoreResult<DiagnosticsSession> {
        match self {
            Store::Memory(store) => store.create_diagnostics_session(session).await,
            Store::Postgres(store) => store.create_diagnostics_session(session).await,
        }
    }

    pub async fn update_diagnostics_session(
        &self,
        session: DiagnosticsSession,
    ) -> StoreResult<DiagnosticsSession> {
        match self {
            Store::Memory(store) => store.update_diagnostics_session(session).await,
            Store::Postgres(store) => store.update_diagnostics_session(session).await,
        }
    }

    pub async fn get_diagnostics_session(
        &self,
        project_id: Id,
        session_id: Id,
    ) -> StoreResult<DiagnosticsSession> {
        match self {
            Store::Memory(store) => store.get_diagnostics_session(project_id, session_id).await,
            Store::Postgres(store) => store.get_diagnostics_session(project_id, session_id).await,
        }
    }

    pub async fn list_diagnostics_sessions(
        &self,
        project_id: Id,
    ) -> StoreResult<Vec<DiagnosticsSession>> {
        match self {
            Store::Memory(store) => Ok(store.list_diagnostics_sessions(project_id).await),
            Store::Postgres(store) => store.list_diagnostics_sessions(project_id).await,
        }
    }

    pub async fn append_audit(&self, audit: AuditLog) -> StoreResult<AuditLog> {
        match self {
            Store::Memory(store) => store.append_audit(audit).await,
            Store::Postgres(store) => store.append_audit(audit).await,
        }
    }

    pub async fn list_audit(
        &self,
        org_id: Id,
        project_id: Option<Id>,
    ) -> StoreResult<Vec<AuditLog>> {
        match self {
            Store::Memory(store) => Ok(store.list_audit(org_id, project_id).await),
            Store::Postgres(store) => store.list_audit(org_id, project_id).await,
        }
    }
}
