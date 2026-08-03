use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use excalibur_domain::{
    Action, ActionDispatchTarget, ActionState, ActionStatusUpdate, ActionTargetStatusChange,
    ActionTargetTransition, AlertEvent, AlertEventState, AlertRule, ApiKey, AuditLog,
    CertificateStatus, Dashboard, Device, DeviceCertificate, DeviceStatus, DiagnosticsSession,
    FirmwareArtifact, FirmwareRollout, Id, Membership, Org, Project, ProjectFeature,
    RemoteShellSession, RemoteShellSessionState, Role, StreamDefinition, TelemetryAggregateBucket,
    TelemetryPoint, User, UserSession,
};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    StoreError, StoreResult,
    actions::{action_status_allowed_source_states, aggregate_action_state},
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
    project_features: HashMap<(Id, String), ProjectFeature>,
    devices: HashMap<Id, Device>,
    certificates: HashMap<Id, DeviceCertificate>,
    streams: HashMap<Id, StreamDefinition>,
    telemetry: Vec<TelemetryPoint>,
    telemetry_sequences: HashSet<TelemetryDedupeKey>,
    actions: HashMap<Id, Action>,
    action_targets: HashMap<(Id, Id), ActionTargetRecord>,
    remote_shell_sessions: HashMap<Id, RemoteShellSession>,
    firmware: HashMap<Id, FirmwareArtifact>,
    firmware_rollouts: HashMap<Id, FirmwareRollout>,
    alerts: HashMap<Id, AlertRule>,
    alert_events: HashMap<Id, AlertEvent>,
    dashboards: HashMap<Id, Dashboard>,
    diagnostics_sessions: HashMap<Id, DiagnosticsSession>,
    audit: Vec<AuditLog>,
}

#[derive(Debug, Clone)]
struct ActionTargetRecord {
    state: ActionState,
    progress: u8,
    errors: Vec<String>,
    updated_at: DateTime<Utc>,
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

    pub async fn list_project_features(&self, project_id: Id) -> Vec<ProjectFeature> {
        let state = self.state.read().await;
        state
            .project_features
            .values()
            .filter(|feature| feature.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn get_project_feature(
        &self,
        project_id: Id,
        feature: &str,
    ) -> StoreResult<Option<ProjectFeature>> {
        let state = self.state.read().await;
        if !state.projects.contains_key(&project_id) {
            return Err(StoreError::NotFound("project"));
        }
        Ok(state
            .project_features
            .get(&(project_id, feature.to_owned()))
            .cloned())
    }

    pub async fn set_project_feature(
        &self,
        project_id: Id,
        feature: &str,
        enabled: bool,
        updated_by: Option<Id>,
        ts: DateTime<Utc>,
    ) -> StoreResult<ProjectFeature> {
        if feature.trim().is_empty() {
            return Err(StoreError::Conflict("project feature"));
        }
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&project_id) {
            return Err(StoreError::NotFound("project"));
        }
        if let Some(user_id) = updated_by
            && !state.users.contains_key(&user_id)
        {
            return Err(StoreError::NotFound("user"));
        }
        let key = (project_id, feature.to_owned());
        let next = match state.project_features.get(&key) {
            Some(existing) => ProjectFeature {
                enabled,
                updated_by,
                updated_at: ts,
                ..existing.clone()
            },
            None => ProjectFeature {
                project_id,
                feature: feature.to_owned(),
                enabled,
                updated_by,
                created_at: ts,
                updated_at: ts,
            },
        };
        state.project_features.insert(key, next.clone());
        Ok(next)
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
    ) -> Vec<TelemetryAggregateBucket> {
        #[derive(Debug, Clone)]
        struct Accumulator {
            count: i64,
            numeric_count: i64,
            sum: f64,
            min: Option<f64>,
            max: Option<f64>,
            last: Option<f64>,
            last_ts: Option<DateTime<Utc>>,
        }

        let bucket_seconds = bucket_seconds.max(1);
        let state = self.state.read().await;
        let mut buckets: BTreeMap<i64, Accumulator> = BTreeMap::new();
        for point in state
            .telemetry
            .iter()
            .filter(|point| point.project_id == project_id)
            .filter(|point| device_id.is_none_or(|id| point.device_id == id))
            .filter(|point| point.stream == stream)
            .filter(|point| point.ts >= from && point.ts < to)
        {
            let bucket_epoch = point.ts.timestamp().div_euclid(bucket_seconds) * bucket_seconds;
            let entry = buckets.entry(bucket_epoch).or_insert(Accumulator {
                count: 0,
                numeric_count: 0,
                sum: 0.0,
                min: None,
                max: None,
                last: None,
                last_ts: None,
            });
            entry.count += 1;
            let numeric = field
                .and_then(|field| point.payload.get(field))
                .and_then(serde_json::Value::as_f64);
            if let Some(value) = numeric {
                entry.numeric_count += 1;
                entry.sum += value;
                entry.min = Some(entry.min.map_or(value, |current| current.min(value)));
                entry.max = Some(entry.max.map_or(value, |current| current.max(value)));
                if entry.last_ts.is_none_or(|last_ts| point.ts >= last_ts) {
                    entry.last = Some(value);
                    entry.last_ts = Some(point.ts);
                }
            }
        }

        buckets
            .into_iter()
            .rev()
            .take(limit)
            .filter_map(|(bucket_epoch, accumulator)| {
                let bucket_start = DateTime::<Utc>::from_timestamp(bucket_epoch, 0)?;
                Some(TelemetryAggregateBucket {
                    project_id,
                    device_id,
                    stream: stream.to_owned(),
                    field: field.map(str::to_owned),
                    bucket_start,
                    bucket_seconds,
                    count: accumulator.count,
                    min: accumulator.min,
                    max: accumulator.max,
                    avg: (accumulator.numeric_count > 0)
                        .then(|| accumulator.sum / accumulator.numeric_count as f64),
                    last: accumulator.last,
                })
            })
            .collect()
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
                    updated_at: action.updated_at,
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
        let allowed_source_states = action_status_allowed_source_states(&update.state);
        if allowed_source_states
            .iter()
            .any(|state| state == &target.state)
        {
            target.state = update.state;
            target.progress = update.progress.min(100);
            target.errors = update.errors;
            target.updated_at = update.ts;
        }
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

    pub async fn claim_queued_action_targets(
        &self,
        limit: usize,
    ) -> StoreResult<Vec<ActionDispatchTarget>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut state = self.state.write().await;
        let mut keys = state
            .action_targets
            .iter()
            .filter(|(_, target)| target.state == ActionState::Queued)
            .map(|(key, target)| (target.updated_at, *key))
            .collect::<Vec<_>>();
        keys.sort();
        keys.truncate(limit);

        let now = Utc::now();
        let mut dispatch_targets = Vec::with_capacity(keys.len());
        let mut affected_actions = HashSet::new();
        for (_, (action_id, device_id)) in keys {
            let Some(action) = state.actions.get(&action_id).cloned() else {
                continue;
            };
            let Some(target) = state.action_targets.get_mut(&(action_id, device_id)) else {
                continue;
            };
            if target.state != ActionState::Queued {
                continue;
            }
            target.state = ActionState::Running;
            target.progress = 0;
            target.errors.clear();
            target.updated_at = now;
            affected_actions.insert(action_id);
            dispatch_targets.push(ActionDispatchTarget {
                project_id: action.project_id,
                action_id,
                device_id,
                name: action.name,
                payload: action.payload,
            });
        }

        for action_id in affected_actions {
            let (state_value, progress, errors) = aggregate_action_targets(
                state
                    .action_targets
                    .iter()
                    .filter(|((candidate_action_id, _), _)| *candidate_action_id == action_id)
                    .map(|(_, target)| target),
            );
            if let Some(action) = state.actions.get_mut(&action_id) {
                action.state = state_value;
                action.progress = progress;
                action.errors = errors;
                action.updated_at = now;
            }
        }

        Ok(dispatch_targets)
    }

    pub async fn get_action_target_state(
        &self,
        project_id: Id,
        action_id: Id,
        device_id: Id,
    ) -> StoreResult<ActionState> {
        let state = self.state.read().await;
        let action = state
            .actions
            .get(&action_id)
            .ok_or(StoreError::NotFound("action"))?;
        if action.project_id != project_id {
            return Err(StoreError::NotFound("action"));
        }
        state
            .action_targets
            .get(&(action_id, device_id))
            .map(|target| target.state.clone())
            .ok_or(StoreError::NotFound("action target"))
    }

    pub async fn transition_action_targets(
        &self,
        transition: ActionTargetTransition,
    ) -> StoreResult<Action> {
        let mut state = self.state.write().await;
        let action = state
            .actions
            .get(&transition.action_id)
            .cloned()
            .ok_or(StoreError::NotFound("action"))?;
        if action.project_id != transition.project_id {
            return Err(StoreError::NotFound("action"));
        }

        let target_device_ids = transition
            .device_ids
            .clone()
            .unwrap_or_else(|| action.device_ids.clone());
        if target_device_ids.is_empty() {
            return Err(StoreError::NotFound("action target"));
        }

        let mut seen_targets = HashSet::new();
        for device_id in &target_device_ids {
            if !seen_targets.insert(*device_id) {
                return Err(StoreError::Conflict("action target"));
            }
            let device = state
                .devices
                .get(device_id)
                .ok_or(StoreError::NotFound("device"))?;
            if device.project_id != transition.project_id {
                return Err(StoreError::TenantScope);
            }
            if !action.device_ids.contains(device_id) {
                return Err(StoreError::NotFound("action target"));
            }
        }

        let mut changed = 0usize;
        for device_id in &target_device_ids {
            let target = state
                .action_targets
                .get_mut(&(transition.action_id, *device_id))
                .ok_or(StoreError::NotFound("action target"))?;
            if transition
                .allowed_source_states
                .iter()
                .any(|state| state == &target.state)
            {
                target.state = transition.next_state.clone();
                if let Some(progress) = transition.progress {
                    target.progress = progress.min(100);
                }
                if let Some(errors) = &transition.errors {
                    target.errors = errors.clone();
                }
                target.updated_at = transition.ts;
                changed += 1;
            }
        }
        if changed == 0 {
            return Err(StoreError::Conflict("action transition"));
        }

        let (state_value, progress, errors) = aggregate_action_targets(
            state
                .action_targets
                .iter()
                .filter(|((action_id, _), _)| *action_id == transition.action_id)
                .map(|(_, target)| target),
        );
        let action = state
            .actions
            .get_mut(&transition.action_id)
            .ok_or(StoreError::NotFound("action"))?;
        action.state = state_value;
        action.progress = progress;
        action.errors = errors;
        action.updated_at = transition.ts;
        Ok(action.clone())
    }

    pub async fn timeout_running_action_targets(
        &self,
        older_than: DateTime<Utc>,
        limit: usize,
        ts: DateTime<Utc>,
    ) -> StoreResult<Vec<ActionTargetStatusChange>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut state = self.state.write().await;
        let mut keys = state
            .action_targets
            .iter()
            .filter(|(_, target)| {
                target.state == ActionState::Running && target.updated_at < older_than
            })
            .map(|((action_id, device_id), target)| (target.updated_at, *action_id, *device_id))
            .collect::<Vec<_>>();
        keys.sort();
        keys.truncate(limit);

        let mut changes = Vec::with_capacity(keys.len());
        let mut affected_actions = HashSet::new();
        for (_, action_id, device_id) in keys {
            let Some(action) = state.actions.get(&action_id).cloned() else {
                continue;
            };
            let Some(target) = state.action_targets.get_mut(&(action_id, device_id)) else {
                continue;
            };
            if target.state != ActionState::Running || target.updated_at >= older_than {
                continue;
            }
            target.state = ActionState::TimedOut;
            target.errors = vec!["action timed out".to_owned()];
            target.updated_at = ts;
            affected_actions.insert(action_id);
            changes.push(ActionTargetStatusChange {
                project_id: action.project_id,
                action_id,
                device_id,
                state: ActionState::TimedOut,
            });
        }

        for action_id in affected_actions {
            let (state_value, progress, errors) = aggregate_action_targets(
                state
                    .action_targets
                    .iter()
                    .filter(|((candidate_action_id, _), _)| *candidate_action_id == action_id)
                    .map(|(_, target)| target),
            );
            if let Some(action) = state.actions.get_mut(&action_id) {
                action.state = state_value;
                action.progress = progress;
                action.errors = errors;
                action.updated_at = ts;
            }
        }

        Ok(changes)
    }

    pub async fn create_remote_shell_session(
        &self,
        session: RemoteShellSession,
    ) -> StoreResult<RemoteShellSession> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&session.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        let device = state
            .devices
            .get(&session.device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != session.project_id {
            return Err(StoreError::NotFound("device"));
        }
        if session.is_open(Utc::now())
            && state.remote_shell_sessions.values().any(|existing| {
                existing.project_id == session.project_id
                    && existing.device_id == session.device_id
                    && existing.is_open(Utc::now())
            })
        {
            return Err(StoreError::Conflict("remote shell session"));
        }
        if let Some(action_id) = session.action_id {
            let action = state
                .actions
                .get(&action_id)
                .ok_or(StoreError::NotFound("action"))?;
            if action.project_id != session.project_id {
                return Err(StoreError::NotFound("action"));
            }
        }
        state
            .remote_shell_sessions
            .insert(session.id, session.clone());
        Ok(session)
    }

    pub async fn attach_remote_shell_action(
        &self,
        project_id: Id,
        session_id: Id,
        action_id: Id,
    ) -> StoreResult<RemoteShellSession> {
        let mut state = self.state.write().await;
        let action = state
            .actions
            .get(&action_id)
            .ok_or(StoreError::NotFound("action"))?;
        if action.project_id != project_id {
            return Err(StoreError::NotFound("action"));
        }
        let session = state
            .remote_shell_sessions
            .get_mut(&session_id)
            .ok_or(StoreError::NotFound("remote shell session"))?;
        if session.project_id != project_id {
            return Err(StoreError::NotFound("remote shell session"));
        }
        session.action_id = Some(action_id);
        session.last_activity_at = Utc::now();
        Ok(session.clone())
    }

    pub async fn get_remote_shell_session(
        &self,
        session_id: Id,
    ) -> StoreResult<RemoteShellSession> {
        let state = self.state.read().await;
        state
            .remote_shell_sessions
            .get(&session_id)
            .cloned()
            .ok_or(StoreError::NotFound("remote shell session"))
    }

    pub async fn list_remote_shell_sessions(&self, project_id: Id) -> Vec<RemoteShellSession> {
        let state = self.state.read().await;
        state
            .remote_shell_sessions
            .values()
            .filter(|session| session.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn find_active_remote_shell_session_for_device(
        &self,
        project_id: Id,
        device_id: Id,
        now: DateTime<Utc>,
    ) -> StoreResult<Option<RemoteShellSession>> {
        let state = self.state.read().await;
        let device = state
            .devices
            .get(&device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != project_id {
            return Err(StoreError::NotFound("device"));
        }
        Ok(state
            .remote_shell_sessions
            .values()
            .find(|session| {
                session.project_id == project_id
                    && session.device_id == device_id
                    && session.is_open(now)
            })
            .cloned())
    }

    pub async fn count_active_remote_shell_sessions(
        &self,
        project_id: Id,
        now: DateTime<Utc>,
    ) -> i64 {
        let state = self.state.read().await;
        state
            .remote_shell_sessions
            .values()
            .filter(|session| session.project_id == project_id && session.is_open(now))
            .count() as i64
    }

    pub async fn mark_remote_shell_session_active(
        &self,
        session_id: Id,
        ts: DateTime<Utc>,
    ) -> StoreResult<RemoteShellSession> {
        let mut state = self.state.write().await;
        let session = state
            .remote_shell_sessions
            .get_mut(&session_id)
            .ok_or(StoreError::NotFound("remote shell session"))?;
        if session.closed_at.is_some() {
            return Err(StoreError::NotFound("remote shell session"));
        }
        session.state = RemoteShellSessionState::Active;
        session.last_activity_at = ts;
        Ok(session.clone())
    }

    pub async fn record_remote_shell_session_bytes(
        &self,
        session_id: Id,
        bytes_from_operator: i64,
        bytes_from_device: i64,
        ts: DateTime<Utc>,
    ) -> StoreResult<RemoteShellSession> {
        let mut state = self.state.write().await;
        let session = state
            .remote_shell_sessions
            .get_mut(&session_id)
            .ok_or(StoreError::NotFound("remote shell session"))?;
        session.bytes_from_operator += bytes_from_operator.max(0);
        session.bytes_from_device += bytes_from_device.max(0);
        session.last_activity_at = ts;
        Ok(session.clone())
    }

    pub async fn close_remote_shell_session(
        &self,
        session_id: Id,
        state_value: RemoteShellSessionState,
        reason: &str,
        ts: DateTime<Utc>,
    ) -> StoreResult<RemoteShellSession> {
        let mut state = self.state.write().await;
        let session = state
            .remote_shell_sessions
            .get_mut(&session_id)
            .ok_or(StoreError::NotFound("remote shell session"))?;
        if session.closed_at.is_none() {
            session.state = state_value;
            session.closed_at = Some(ts);
            session.close_reason = Some(reason.to_owned());
        }
        session.last_activity_at = ts;
        Ok(session.clone())
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

    pub async fn finalize_firmware(
        &self,
        project_id: Id,
        firmware_id: Id,
        sha256: &str,
        size_bytes: i64,
        signature: Option<&str>,
        ts: DateTime<Utc>,
    ) -> StoreResult<FirmwareArtifact> {
        let mut state = self.state.write().await;
        let artifact = state
            .firmware
            .get_mut(&firmware_id)
            .ok_or(StoreError::NotFound("firmware"))?;
        if artifact.project_id != project_id {
            return Err(StoreError::NotFound("firmware"));
        }
        if artifact.sha256 != sha256
            || artifact.size_bytes != size_bytes
            || artifact.signature.as_deref() != signature
        {
            return Err(StoreError::Conflict("firmware verification"));
        }
        artifact.uploaded_at = Some(ts);
        artifact.verified_at = Some(ts);
        artifact.active = true;
        Ok(artifact.clone())
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

    pub async fn create_firmware_rollout(
        &self,
        rollout: FirmwareRollout,
    ) -> StoreResult<FirmwareRollout> {
        let mut state = self.state.write().await;
        let artifact = state
            .firmware
            .get(&rollout.firmware_id)
            .ok_or(StoreError::NotFound("firmware"))?;
        if artifact.project_id != rollout.project_id {
            return Err(StoreError::NotFound("firmware"));
        }
        let action = state
            .actions
            .get(&rollout.action_id)
            .ok_or(StoreError::NotFound("action"))?;
        if action.project_id != rollout.project_id {
            return Err(StoreError::NotFound("action"));
        }
        if rollout.cohort_size <= 0 {
            return Err(StoreError::Conflict("firmware rollout"));
        }
        state.firmware_rollouts.insert(rollout.id, rollout.clone());
        Ok(rollout)
    }

    pub async fn list_firmware_rollouts(&self, project_id: Id) -> Vec<FirmwareRollout> {
        let state = self.state.read().await;
        state
            .firmware_rollouts
            .values()
            .filter(|rollout| rollout.project_id == project_id)
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

    pub async fn list_enabled_alerts(&self) -> Vec<AlertRule> {
        let state = self.state.read().await;
        state
            .alerts
            .values()
            .filter(|alert| alert.enabled)
            .cloned()
            .collect()
    }

    pub async fn upsert_firing_alert_event(&self, event: AlertEvent) -> StoreResult<AlertEvent> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&event.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        let alert = state
            .alerts
            .get(&event.alert_rule_id)
            .ok_or(StoreError::NotFound("alert"))?;
        if alert.project_id != event.project_id {
            return Err(StoreError::NotFound("alert"));
        }
        if let Some(device_id) = event.device_id {
            let device = state
                .devices
                .get(&device_id)
                .ok_or(StoreError::NotFound("device"))?;
            if device.project_id != event.project_id {
                return Err(StoreError::NotFound("device"));
            }
        }

        let existing_id = state
            .alert_events
            .iter()
            .find(|(_, existing)| {
                existing.project_id == event.project_id
                    && existing.alert_rule_id == event.alert_rule_id
                    && existing.dedupe_key == event.dedupe_key
                    && existing.resolved_at.is_none()
            })
            .map(|(id, _)| *id);
        if let Some(existing_id) = existing_id {
            let existing = state
                .alert_events
                .get_mut(&existing_id)
                .ok_or(StoreError::NotFound("alert event"))?;
            existing.state = AlertEventState::Firing;
            existing.message = event.message;
            existing.observed_value = event.observed_value;
            existing.threshold = event.threshold;
            existing.last_seen_at = event.last_seen_at;
            existing.last_notification_error = None;
            Ok(existing.clone())
        } else {
            state.alert_events.insert(event.id, event.clone());
            Ok(event)
        }
    }

    pub async fn resolve_alert_event(
        &self,
        project_id: Id,
        alert_rule_id: Id,
        dedupe_key: &str,
        ts: DateTime<Utc>,
    ) -> StoreResult<Option<AlertEvent>> {
        let mut state = self.state.write().await;
        let existing_id = state
            .alert_events
            .iter()
            .find(|(_, existing)| {
                existing.project_id == project_id
                    && existing.alert_rule_id == alert_rule_id
                    && existing.dedupe_key == dedupe_key
                    && existing.resolved_at.is_none()
            })
            .map(|(id, _)| *id);
        let Some(existing_id) = existing_id else {
            return Ok(None);
        };
        let existing = state
            .alert_events
            .get_mut(&existing_id)
            .ok_or(StoreError::NotFound("alert event"))?;
        existing.state = AlertEventState::Resolved;
        existing.resolved_at = Some(ts);
        existing.last_seen_at = ts;
        Ok(Some(existing.clone()))
    }

    pub async fn list_alert_events(
        &self,
        project_id: Id,
        state_filter: Option<AlertEventState>,
    ) -> Vec<AlertEvent> {
        let state = self.state.read().await;
        state
            .alert_events
            .values()
            .filter(|event| event.project_id == project_id)
            .filter(|event| {
                state_filter
                    .as_ref()
                    .is_none_or(|expected| &event.state == expected)
            })
            .cloned()
            .collect()
    }

    pub async fn record_alert_notification_attempt(
        &self,
        project_id: Id,
        alert_event_id: Id,
        error: Option<String>,
        ts: DateTime<Utc>,
    ) -> StoreResult<AlertEvent> {
        let mut state = self.state.write().await;
        let event = state
            .alert_events
            .get_mut(&alert_event_id)
            .ok_or(StoreError::NotFound("alert event"))?;
        if event.project_id != project_id {
            return Err(StoreError::NotFound("alert event"));
        }
        event.notification_attempts += 1;
        event.last_notification_error = error;
        event.last_seen_at = event.last_seen_at.max(ts);
        Ok(event.clone())
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

    pub async fn create_diagnostics_session(
        &self,
        session: DiagnosticsSession,
    ) -> StoreResult<DiagnosticsSession> {
        let mut state = self.state.write().await;
        let device = state
            .devices
            .get(&session.device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != session.project_id {
            return Err(StoreError::NotFound("device"));
        }
        if let Some(action_id) = session.action_id {
            let action = state
                .actions
                .get(&action_id)
                .ok_or(StoreError::NotFound("action"))?;
            if action.project_id != session.project_id {
                return Err(StoreError::NotFound("action"));
            }
        }
        state
            .diagnostics_sessions
            .insert(session.id, session.clone());
        Ok(session)
    }

    pub async fn update_diagnostics_session(
        &self,
        session: DiagnosticsSession,
    ) -> StoreResult<DiagnosticsSession> {
        let mut state = self.state.write().await;
        let existing = state
            .diagnostics_sessions
            .get_mut(&session.id)
            .ok_or(StoreError::NotFound("diagnostics session"))?;
        if existing.project_id != session.project_id || existing.device_id != session.device_id {
            return Err(StoreError::NotFound("diagnostics session"));
        }
        *existing = session.clone();
        Ok(session)
    }

    pub async fn get_diagnostics_session(
        &self,
        project_id: Id,
        session_id: Id,
    ) -> StoreResult<DiagnosticsSession> {
        let state = self.state.read().await;
        let session = state
            .diagnostics_sessions
            .get(&session_id)
            .ok_or(StoreError::NotFound("diagnostics session"))?;
        if session.project_id != project_id {
            return Err(StoreError::NotFound("diagnostics session"));
        }
        Ok(session.clone())
    }

    pub async fn list_diagnostics_sessions(&self, project_id: Id) -> Vec<DiagnosticsSession> {
        let state = self.state.read().await;
        state
            .diagnostics_sessions
            .values()
            .filter(|session| session.project_id == project_id)
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
