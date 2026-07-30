use excalibur_domain::{
    Action, ActionStatusUpdate, AlertRule, ApiKey, AuditLog, Dashboard, Device, DeviceCertificate,
    FirmwareArtifact, Id, Membership, Org, Project, Role, StreamDefinition, TelemetryPoint, User,
    UserSession,
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

    pub async fn create_firmware(
        &self,
        artifact: FirmwareArtifact,
    ) -> StoreResult<FirmwareArtifact> {
        match self {
            Store::Memory(store) => store.create_firmware(artifact).await,
            Store::Postgres(store) => store.create_firmware(artifact).await,
        }
    }

    pub async fn list_firmware(&self, project_id: Id) -> StoreResult<Vec<FirmwareArtifact>> {
        match self {
            Store::Memory(store) => Ok(store.list_firmware(project_id).await),
            Store::Postgres(store) => store.list_firmware(project_id).await,
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
