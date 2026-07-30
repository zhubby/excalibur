use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use chrono::Utc;
use excalibur_domain::{
    Action, ActionState, ActionStatusUpdate, AlertRule, ApiKey, AuditLog, CertificateStatus,
    Dashboard, Device, DeviceCertificate, DeviceStatus, FirmwareArtifact, Id, Membership, Org,
    Project, Role, StreamDefinition, TelemetryPoint, User, UserSession,
};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    StoreError, StoreResult,
    actions::aggregate_action_state,
    telemetry::{TelemetryDedupeKey, telemetry_dedupe_key},
};

#[derive(Debug, Default)]
struct MemoryState {
    users: HashMap<Id, User>,
    users_by_email: HashMap<String, Id>,
    sessions: HashMap<Id, UserSession>,
    sessions_by_token_hash: HashMap<String, Id>,
    sessions_by_refresh_hash: HashMap<String, Id>,
    used_refresh_tokens: HashMap<String, Id>,
    api_keys: HashMap<Id, ApiKey>,
    api_keys_by_hash: HashMap<String, Id>,
    orgs: HashMap<Id, Org>,
    memberships: Vec<Membership>,
    projects: HashMap<Id, Project>,
    devices: HashMap<Id, Device>,
    certificates: HashMap<Id, DeviceCertificate>,
    streams: HashMap<Id, StreamDefinition>,
    telemetry: Vec<TelemetryPoint>,
    telemetry_sequences: HashSet<TelemetryDedupeKey>,
    actions: HashMap<Id, Action>,
    action_targets: HashMap<(Id, Id), ActionTargetRecord>,
    firmware: HashMap<Id, FirmwareArtifact>,
    alerts: HashMap<Id, AlertRule>,
    dashboards: HashMap<Id, Dashboard>,
    audit: Vec<AuditLog>,
}

#[derive(Debug, Clone)]
struct ActionTargetRecord {
    state: ActionState,
    progress: u8,
    errors: Vec<String>,
}

fn aggregate_action_targets<'a>(
    targets: impl Iterator<Item = &'a ActionTargetRecord>,
) -> (ActionState, u8, Vec<String>) {
    let mut target_count = 0usize;
    let mut completed_count = 0usize;
    let mut failed_count = 0usize;
    let mut timed_out_count = 0usize;
    let mut cancelled_count = 0usize;
    let mut running_count = 0usize;
    let mut waiting_count = 0usize;
    let mut progress_total = 0usize;
    let mut errors = Vec::new();

    for target in targets {
        target_count += 1;
        progress_total += target.progress as usize;
        errors.extend(target.errors.clone());
        match &target.state {
            ActionState::Queued => {}
            ActionState::WaitingApproval => waiting_count += 1,
            ActionState::Running => running_count += 1,
            ActionState::Completed => completed_count += 1,
            ActionState::Failed => failed_count += 1,
            ActionState::Cancelled => cancelled_count += 1,
            ActionState::TimedOut => timed_out_count += 1,
        }
    }

    let state = aggregate_action_state(
        target_count,
        completed_count,
        failed_count,
        timed_out_count,
        cancelled_count,
        running_count,
        waiting_count,
    );
    let progress = progress_total
        .checked_div(target_count)
        .unwrap_or(0)
        .min(100) as u8;
    (state, progress, errors)
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

    pub async fn create_session(&self, session: UserSession) -> StoreResult<UserSession> {
        let mut state = self.state.write().await;
        if !state.users.contains_key(&session.user_id) {
            return Err(StoreError::NotFound("user"));
        }
        if state
            .sessions_by_token_hash
            .contains_key(&session.token_hash)
            || state
                .sessions_by_refresh_hash
                .contains_key(&session.refresh_token_hash)
        {
            return Err(StoreError::Conflict("session"));
        }
        state
            .sessions_by_token_hash
            .insert(session.token_hash.clone(), session.id);
        state
            .sessions_by_refresh_hash
            .insert(session.refresh_token_hash.clone(), session.id);
        state.sessions.insert(session.id, session.clone());
        Ok(session)
    }

    pub async fn get_active_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> StoreResult<UserSession> {
        let mut state = self.state.write().await;
        let session_id = state
            .sessions_by_token_hash
            .get(token_hash)
            .copied()
            .ok_or(StoreError::NotFound("session"))?;
        let session = state
            .sessions
            .get_mut(&session_id)
            .ok_or(StoreError::NotFound("session"))?;
        if session.revoked_at.is_some() || session.expires_at <= Utc::now() {
            return Err(StoreError::NotFound("session"));
        }
        session.last_used_at = Some(Utc::now());
        Ok(session.clone())
    }

    pub async fn rotate_session_refresh_token(
        &self,
        refresh_token_hash: &str,
        next_token_hash: String,
        next_refresh_token_hash: String,
        next_expires_at: chrono::DateTime<Utc>,
        next_refresh_expires_at: chrono::DateTime<Utc>,
    ) -> StoreResult<UserSession> {
        let mut state = self.state.write().await;
        if let Some(session_id) = state.used_refresh_tokens.get(refresh_token_hash).copied() {
            if let Some(session) = state.sessions.get_mut(&session_id) {
                session.revoked_at = Some(Utc::now());
            }
            return Err(StoreError::Conflict("refresh token reuse"));
        }
        if state.sessions_by_token_hash.contains_key(&next_token_hash)
            || state
                .sessions_by_refresh_hash
                .contains_key(&next_refresh_token_hash)
        {
            return Err(StoreError::Conflict("session"));
        }
        let session_id = state
            .sessions_by_refresh_hash
            .get(refresh_token_hash)
            .copied()
            .ok_or(StoreError::NotFound("refresh token"))?;
        let (old_token_hash, old_refresh_hash) = {
            let session = state
                .sessions
                .get(&session_id)
                .ok_or(StoreError::NotFound("session"))?;
            if session.revoked_at.is_some() || session.refresh_expires_at <= Utc::now() {
                return Err(StoreError::NotFound("refresh token"));
            }
            (
                session.token_hash.clone(),
                session.refresh_token_hash.clone(),
            )
        };
        state.sessions_by_token_hash.remove(&old_token_hash);
        state.sessions_by_refresh_hash.remove(&old_refresh_hash);
        state
            .used_refresh_tokens
            .insert(old_refresh_hash, session_id);
        state
            .sessions_by_token_hash
            .insert(next_token_hash.clone(), session_id);
        state
            .sessions_by_refresh_hash
            .insert(next_refresh_token_hash.clone(), session_id);
        let session = state
            .sessions
            .get_mut(&session_id)
            .ok_or(StoreError::NotFound("session"))?;
        session.token_hash = next_token_hash;
        session.refresh_token_hash = next_refresh_token_hash;
        session.expires_at = next_expires_at;
        session.refresh_expires_at = next_refresh_expires_at;
        session.last_used_at = Some(Utc::now());
        Ok(session.clone())
    }

    pub async fn revoke_session_by_token_hash(&self, token_hash: &str) -> StoreResult<()> {
        let mut state = self.state.write().await;
        let session_id = state
            .sessions_by_token_hash
            .get(token_hash)
            .copied()
            .ok_or(StoreError::NotFound("session"))?;
        let session = state
            .sessions
            .get_mut(&session_id)
            .ok_or(StoreError::NotFound("session"))?;
        session.revoked_at = Some(Utc::now());
        Ok(())
    }

    pub async fn create_api_key(&self, api_key: ApiKey) -> StoreResult<ApiKey> {
        let mut state = self.state.write().await;
        if !state.orgs.contains_key(&api_key.org_id) {
            return Err(StoreError::NotFound("org"));
        }
        if let Some(project_id) = api_key.project_id {
            let project = state
                .projects
                .get(&project_id)
                .ok_or(StoreError::NotFound("project"))?;
            if project.org_id != api_key.org_id {
                return Err(StoreError::TenantScope);
            }
        }
        if api_key.scopes.iter().any(|scope| scope.trim().is_empty()) {
            return Err(StoreError::Conflict("api key scope"));
        }
        if state.api_keys_by_hash.contains_key(&api_key.key_hash) {
            return Err(StoreError::Conflict("api key"));
        }
        state
            .api_keys_by_hash
            .insert(api_key.key_hash.clone(), api_key.id);
        state.api_keys.insert(api_key.id, api_key.clone());
        Ok(api_key)
    }

    pub async fn get_active_api_key_by_hash(&self, key_hash: &str) -> StoreResult<ApiKey> {
        let mut state = self.state.write().await;
        let key_id = state
            .api_keys_by_hash
            .get(key_hash)
            .copied()
            .ok_or(StoreError::NotFound("api key"))?;
        let api_key = state
            .api_keys
            .get_mut(&key_id)
            .ok_or(StoreError::NotFound("api key"))?;
        if api_key.revoked_at.is_some()
            || api_key
                .expires_at
                .is_some_and(|expires_at| expires_at <= Utc::now())
        {
            return Err(StoreError::NotFound("api key"));
        }
        api_key.last_used_at = Some(Utc::now());
        Ok(api_key.clone())
    }

    pub async fn list_api_keys(&self, org_id: Id, project_id: Option<Id>) -> Vec<ApiKey> {
        let state = self.state.read().await;
        state
            .api_keys
            .values()
            .filter(|api_key| api_key.org_id == org_id)
            .filter(|api_key| project_id.is_none_or(|id| api_key.project_id == Some(id)))
            .cloned()
            .collect()
    }

    pub async fn revoke_api_key(&self, org_id: Id, api_key_id: Id) -> StoreResult<ApiKey> {
        let mut state = self.state.write().await;
        let api_key = state
            .api_keys
            .get_mut(&api_key_id)
            .ok_or(StoreError::NotFound("api key"))?;
        if api_key.org_id != org_id {
            return Err(StoreError::NotFound("api key"));
        }
        api_key.revoked_at = Some(Utc::now());
        Ok(api_key.clone())
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
        state
            .devices
            .values()
            .find(|device| device.project_id == project_id && device.id == device_id)
            .cloned()
            .ok_or(StoreError::NotFound("device"))
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
            return Err(StoreError::NotFound("device"));
        }
        if state
            .certificates
            .values()
            .any(|existing| existing.fingerprint_sha256 == certificate.fingerprint_sha256)
        {
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

    pub async fn get_active_device_by_certificate_fingerprint(
        &self,
        fingerprint_sha256: &str,
    ) -> StoreResult<Device> {
        let state = self.state.read().await;
        let certificate = state
            .certificates
            .values()
            .find(|certificate| certificate.fingerprint_sha256 == fingerprint_sha256)
            .ok_or(StoreError::NotFound("certificate"))?;
        if !matches!(certificate.status, CertificateStatus::Active)
            || certificate.not_before > Utc::now()
            || certificate.not_after <= Utc::now()
        {
            return Err(StoreError::NotFound("certificate"));
        }
        let device = state
            .devices
            .get(&certificate.device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != certificate.project_id
            || matches!(device.status, DeviceStatus::Disabled)
        {
            return Err(StoreError::NotFound("certificate"));
        }
        Ok(device.clone())
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
            return Err(StoreError::NotFound("device"));
        }
        let certificate = state
            .certificates
            .get_mut(&certificate_id)
            .ok_or(StoreError::NotFound("certificate"))?;
        if certificate.project_id != project_id || certificate.device_id != device_id {
            return Err(StoreError::NotFound("certificate"));
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
            return Err(StoreError::NotFound("device"));
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
            return Err(StoreError::NotFound("device"));
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
                return Err(StoreError::NotFound("device"));
            }
        }
        let mut written = 0;
        for point in points {
            if state
                .telemetry_sequences
                .insert(telemetry_dedupe_key(&point))
            {
                state.telemetry.push(point);
                written += 1;
            }
        }
        Ok(written)
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
                return Err(StoreError::NotFound("device"));
            }
        }
        for device_id in &action.device_ids {
            state.action_targets.insert(
                (action.id, *device_id),
                ActionTargetRecord {
                    state: action.state.clone(),
                    progress: action.progress,
                    errors: action.errors.clone(),
                },
            );
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
            return Err(StoreError::NotFound("action"));
        }
        let device = state
            .devices
            .get(&update.device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != update.project_id {
            return Err(StoreError::NotFound("device"));
        }
        if !action.device_ids.contains(&update.device_id) {
            return Err(StoreError::NotFound("action target"));
        }

        let target = state
            .action_targets
            .get_mut(&(update.action_id, update.device_id))
            .ok_or(StoreError::NotFound("action target"))?;
        target.state = update.state;
        target.progress = update.progress.min(100);
        target.errors = update.errors;
        let (state_value, progress, errors) = aggregate_action_targets(
            state
                .action_targets
                .iter()
                .filter(|((action_id, _), _)| *action_id == update.action_id)
                .map(|(_, target)| target),
        );
        let action = state
            .actions
            .get_mut(&update.action_id)
            .ok_or(StoreError::NotFound("action"))?;
        action.state = state_value;
        action.progress = progress;
        action.errors = errors;
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

    pub async fn append_audit(&self, audit: AuditLog) -> StoreResult<AuditLog> {
        let mut state = self.state.write().await;
        if !state.orgs.contains_key(&audit.org_id) {
            return Err(StoreError::NotFound("org"));
        }
        if let Some(project_id) = audit.project_id {
            let project = state
                .projects
                .get(&project_id)
                .ok_or(StoreError::NotFound("project"))?;
            if project.org_id != audit.org_id {
                return Err(StoreError::TenantScope);
            }
        }
        state.audit.push(audit.clone());
        Ok(audit)
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
