use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use chrono::Utc;
use excalibur_domain::{
    Action, ActionState, ActionStatusUpdate, AlertRule, AuditLog, CertificateStatus, Dashboard,
    Device, DeviceCertificate, DeviceStatus, FirmwareArtifact, Id, Membership, Org, Project, Role,
    StreamDefinition, TelemetryPoint, User,
};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("{0} was not found")]
    NotFound(&'static str),
    #[error("{0} already exists")]
    Conflict(&'static str),
    #[error("tenant scope violation")]
    TenantScope,
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Default)]
struct MemoryState {
    users: HashMap<Id, User>,
    users_by_email: HashMap<String, Id>,
    orgs: HashMap<Id, Org>,
    memberships: Vec<Membership>,
    projects: HashMap<Id, Project>,
    devices: HashMap<Id, Device>,
    certificates: HashMap<Id, DeviceCertificate>,
    streams: HashMap<Id, StreamDefinition>,
    telemetry: Vec<TelemetryPoint>,
    actions: HashMap<Id, Action>,
    firmware: HashMap<Id, FirmwareArtifact>,
    alerts: HashMap<Id, AlertRule>,
    dashboards: HashMap<Id, Dashboard>,
    audit: Vec<AuditLog>,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    state: Arc<RwLock<MemoryState>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create_user(&self, user: User) -> StoreResult<User> {
        let mut state = self.state.write().await;
        let email_key = user.email.to_lowercase();
        if state.users_by_email.contains_key(&email_key) {
            return Err(StoreError::Conflict("user"));
        }
        state.users_by_email.insert(email_key, user.id);
        state.users.insert(user.id, user.clone());
        Ok(user)
    }

    pub async fn get_user_by_email(&self, email: &str) -> StoreResult<User> {
        let state = self.state.read().await;
        let id = state
            .users_by_email
            .get(&email.to_lowercase())
            .ok_or(StoreError::NotFound("user"))?;
        state
            .users
            .get(id)
            .cloned()
            .ok_or(StoreError::NotFound("user"))
    }

    pub async fn create_org(&self, org: Org, owner_id: Id) -> StoreResult<Org> {
        let mut state = self.state.write().await;
        if state
            .orgs
            .values()
            .any(|existing| existing.slug == org.slug)
        {
            return Err(StoreError::Conflict("org"));
        }
        state
            .memberships
            .push(Membership::new(org.id, owner_id, Role::Owner));
        state.orgs.insert(org.id, org.clone());
        Ok(org)
    }

    pub async fn add_membership(&self, membership: Membership) -> StoreResult<Membership> {
        let mut state = self.state.write().await;
        if !state.orgs.contains_key(&membership.org_id) {
            return Err(StoreError::NotFound("org"));
        }
        if !state.users.contains_key(&membership.user_id) {
            return Err(StoreError::NotFound("user"));
        }
        if state.memberships.iter().any(|existing| {
            existing.org_id == membership.org_id && existing.user_id == membership.user_id
        }) {
            return Err(StoreError::Conflict("membership"));
        }
        state.memberships.push(membership.clone());
        Ok(membership)
    }

    pub async fn list_orgs_for_user(&self, user_id: Id) -> Vec<Org> {
        let state = self.state.read().await;
        state
            .memberships
            .iter()
            .filter(|membership| membership.user_id == user_id)
            .filter_map(|membership| state.orgs.get(&membership.org_id))
            .cloned()
            .collect()
    }

    pub async fn user_role(&self, org_id: Id, user_id: Id) -> Option<Role> {
        let state = self.state.read().await;
        state
            .memberships
            .iter()
            .find(|membership| membership.org_id == org_id && membership.user_id == user_id)
            .map(|membership| membership.role)
    }

    pub async fn create_project(&self, project: Project) -> StoreResult<Project> {
        let mut state = self.state.write().await;
        if !state.orgs.contains_key(&project.org_id) {
            return Err(StoreError::NotFound("org"));
        }
        if state
            .projects
            .values()
            .any(|existing| existing.org_id == project.org_id && existing.slug == project.slug)
        {
            return Err(StoreError::Conflict("project"));
        }
        state.projects.insert(project.id, project.clone());
        Ok(project)
    }

    pub async fn list_projects(&self, org_id: Id) -> Vec<Project> {
        let state = self.state.read().await;
        state
            .projects
            .values()
            .filter(|project| project.org_id == org_id)
            .cloned()
            .collect()
    }

    pub async fn get_project(&self, project_id: Id) -> StoreResult<Project> {
        let state = self.state.read().await;
        state
            .projects
            .get(&project_id)
            .cloned()
            .ok_or(StoreError::NotFound("project"))
    }

    pub async fn get_project_for_user(&self, project_id: Id, user_id: Id) -> StoreResult<Project> {
        let state = self.state.read().await;
        let project = state
            .projects
            .get(&project_id)
            .cloned()
            .ok_or(StoreError::NotFound("project"))?;
        let has_membership = state
            .memberships
            .iter()
            .any(|membership| membership.org_id == project.org_id && membership.user_id == user_id);
        if has_membership {
            Ok(project)
        } else {
            Err(StoreError::TenantScope)
        }
    }

    pub async fn create_device(&self, device: Device) -> StoreResult<Device> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&device.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        state.devices.insert(device.id, device.clone());
        Ok(device)
    }

    pub async fn list_devices(&self, project_id: Id) -> Vec<Device> {
        let state = self.state.read().await;
        state
            .devices
            .values()
            .filter(|device| device.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn get_device(&self, project_id: Id, device_id: Id) -> StoreResult<Device> {
        let state = self.state.read().await;
        let device = state
            .devices
            .get(&device_id)
            .cloned()
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != project_id {
            return Err(StoreError::TenantScope);
        }
        Ok(device)
    }

    pub async fn create_device_certificate(
        &self,
        certificate: DeviceCertificate,
    ) -> StoreResult<DeviceCertificate> {
        let mut state = self.state.write().await;
        let device = state
            .devices
            .get(&certificate.device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != certificate.project_id {
            return Err(StoreError::TenantScope);
        }
        if state.certificates.values().any(|existing| {
            existing.project_id == certificate.project_id
                && existing.device_id == certificate.device_id
                && existing.fingerprint_sha256 == certificate.fingerprint_sha256
                && existing.status == CertificateStatus::Active
        }) {
            return Err(StoreError::Conflict("certificate"));
        }
        state
            .certificates
            .insert(certificate.id, certificate.clone());
        Ok(certificate)
    }

    pub async fn list_device_certificates(
        &self,
        project_id: Id,
        device_id: Id,
    ) -> StoreResult<Vec<DeviceCertificate>> {
        self.get_device(project_id, device_id).await?;
        let state = self.state.read().await;
        Ok(state
            .certificates
            .values()
            .filter(|certificate| {
                certificate.project_id == project_id && certificate.device_id == device_id
            })
            .cloned()
            .collect())
    }

    pub async fn revoke_device_certificate(
        &self,
        project_id: Id,
        device_id: Id,
        certificate_id: Id,
    ) -> StoreResult<DeviceCertificate> {
        let mut state = self.state.write().await;
        let device = state
            .devices
            .get(&device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != project_id {
            return Err(StoreError::TenantScope);
        }
        let certificate = state
            .certificates
            .get_mut(&certificate_id)
            .ok_or(StoreError::NotFound("certificate"))?;
        if certificate.project_id != project_id || certificate.device_id != device_id {
            return Err(StoreError::TenantScope);
        }
        certificate.revoke();
        Ok(certificate.clone())
    }

    pub async fn touch_device_online(&self, project_id: Id, device_id: Id) -> StoreResult<Device> {
        let mut state = self.state.write().await;
        let device = state
            .devices
            .get_mut(&device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != project_id {
            return Err(StoreError::TenantScope);
        }
        device.status = DeviceStatus::Online;
        device.last_seen_at = Some(Utc::now());
        Ok(device.clone())
    }

    pub async fn update_shadow(
        &self,
        project_id: Id,
        device_id: Id,
        shadow: Value,
    ) -> StoreResult<Device> {
        let mut state = self.state.write().await;
        let device = state
            .devices
            .get_mut(&device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != project_id {
            return Err(StoreError::TenantScope);
        }
        device.latest_shadow = shadow;
        device.last_seen_at = Some(Utc::now());
        Ok(device.clone())
    }

    pub async fn create_stream(&self, stream: StreamDefinition) -> StoreResult<StreamDefinition> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&stream.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        if state.streams.values().any(|existing| {
            existing.project_id == stream.project_id && existing.name == stream.name
        }) {
            return Err(StoreError::Conflict("stream"));
        }
        state.streams.insert(stream.id, stream.clone());
        Ok(stream)
    }

    pub async fn list_streams(&self, project_id: Id) -> Vec<StreamDefinition> {
        let state = self.state.read().await;
        state
            .streams
            .values()
            .filter(|stream| stream.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn write_telemetry(&self, points: Vec<TelemetryPoint>) -> StoreResult<usize> {
        let mut state = self.state.write().await;
        for point in &points {
            let device = state
                .devices
                .get(&point.device_id)
                .ok_or(StoreError::NotFound("device"))?;
            if device.project_id != point.project_id {
                return Err(StoreError::TenantScope);
            }
        }
        let count = points.len();
        state.telemetry.extend(points);
        Ok(count)
    }

    pub async fn query_telemetry(
        &self,
        project_id: Id,
        device_id: Option<Id>,
        stream: Option<&str>,
        limit: usize,
    ) -> Vec<TelemetryPoint> {
        let state = self.state.read().await;
        let mut rows: Vec<TelemetryPoint> = state
            .telemetry
            .iter()
            .filter(|point| point.project_id == project_id)
            .filter(|point| device_id.is_none_or(|id| point.device_id == id))
            .filter(|point| stream.is_none_or(|name| point.stream == name))
            .cloned()
            .collect();
        rows.sort_by_key(|point| point.ts);
        rows.into_iter().rev().take(limit).collect()
    }

    pub async fn create_action(&self, action: Action) -> StoreResult<Action> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&action.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        let mut seen_targets = HashSet::new();
        for device_id in &action.device_ids {
            if !seen_targets.insert(*device_id) {
                return Err(StoreError::Conflict("action target"));
            }
            let device = state
                .devices
                .get(device_id)
                .ok_or(StoreError::NotFound("device"))?;
            if device.project_id != action.project_id {
                return Err(StoreError::TenantScope);
            }
        }
        state.actions.insert(action.id, action.clone());
        Ok(action)
    }

    pub async fn list_actions(&self, project_id: Id) -> Vec<Action> {
        let state = self.state.read().await;
        state
            .actions
            .values()
            .filter(|action| action.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn update_action_status(&self, update: ActionStatusUpdate) -> StoreResult<Action> {
        let mut state = self.state.write().await;
        let action = state
            .actions
            .get(&update.action_id)
            .ok_or(StoreError::NotFound("action"))?;
        if action.project_id != update.project_id {
            return Err(StoreError::TenantScope);
        }
        let device = state
            .devices
            .get(&update.device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != update.project_id {
            return Err(StoreError::TenantScope);
        }
        if !action.device_ids.contains(&update.device_id) {
            return Err(StoreError::TenantScope);
        }

        let action = state
            .actions
            .get_mut(&update.action_id)
            .ok_or(StoreError::NotFound("action"))?;
        action.state = update.state;
        action.progress = update.progress.min(100);
        action.errors = update.errors;
        action.updated_at = update.ts;
        Ok(action.clone())
    }

    pub async fn create_firmware(
        &self,
        artifact: FirmwareArtifact,
    ) -> StoreResult<FirmwareArtifact> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&artifact.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        if state.firmware.values().any(|existing| {
            existing.project_id == artifact.project_id
                && existing.component == artifact.component
                && existing.version == artifact.version
        }) {
            return Err(StoreError::Conflict("firmware"));
        }
        state.firmware.insert(artifact.id, artifact.clone());
        Ok(artifact)
    }

    pub async fn list_firmware(&self, project_id: Id) -> Vec<FirmwareArtifact> {
        let state = self.state.read().await;
        state
            .firmware
            .values()
            .filter(|artifact| artifact.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn create_alert(&self, alert: AlertRule) -> StoreResult<AlertRule> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&alert.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        state.alerts.insert(alert.id, alert.clone());
        Ok(alert)
    }

    pub async fn list_alerts(&self, project_id: Id) -> Vec<AlertRule> {
        let state = self.state.read().await;
        state
            .alerts
            .values()
            .filter(|alert| alert.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn create_dashboard(&self, dashboard: Dashboard) -> StoreResult<Dashboard> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&dashboard.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        state.dashboards.insert(dashboard.id, dashboard.clone());
        Ok(dashboard)
    }

    pub async fn list_dashboards(&self, project_id: Id) -> Vec<Dashboard> {
        let state = self.state.read().await;
        state
            .dashboards
            .values()
            .filter(|dashboard| dashboard.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn append_audit(&self, audit: AuditLog) -> AuditLog {
        let mut state = self.state.write().await;
        state.audit.push(audit.clone());
        audit
    }

    pub async fn list_audit(&self, org_id: Id, project_id: Option<Id>) -> Vec<AuditLog> {
        let state = self.state.read().await;
        state
            .audit
            .iter()
            .filter(|entry| entry.org_id == org_id)
            .filter(|entry| project_id.is_none_or(|id| entry.project_id == Some(id)))
            .cloned()
            .collect()
    }
}

pub fn map_terminal_action_state(state: &str) -> ActionState {
    match state {
        "Completed" | "completed" => ActionState::Completed,
        "Failed" | "failed" => ActionState::Failed,
        "Cancelled" | "cancelled" => ActionState::Cancelled,
        "TimedOut" | "timed_out" => ActionState::TimedOut,
        _ => ActionState::Running,
    }
}

#[cfg(feature = "toasty-control-plane")]
pub mod toasty_boundary {
    //! Toasty integration boundary for control-plane models.
    //!
    //! The first implementation keeps telemetry outside Toasty and uses raw SQL
    //! for Timescale hypertables. Control-plane repositories can replace the
    //! in-memory store behind this boundary without changing API handlers.

    pub type ToastyDb = toasty::Db;
}

#[cfg(test)]
mod tests {
    use super::*;
    use excalibur_domain::{
        Device, Org, Project, StreamDefinition, StreamField, StreamFieldType, TelemetryPoint,
    };
    use serde_json::json;

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
            StoreError::TenantScope
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

        assert_eq!(error, StoreError::TenantScope);
    }

    #[tokio::test]
    async fn mirrors_unique_constraints_for_project_stream_and_firmware() {
        let store = MemoryStore::new();
        let user = store
            .create_user(User::new("unique@example.com", "Unique", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Unique Org", "unique"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
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
    }
}
