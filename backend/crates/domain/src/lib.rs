use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

pub type Id = Uuid;
pub type JsonObject = Map<String, Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TenantContext {
    pub org_id: Id,
    pub project_id: Id,
    pub actor_id: Option<Id>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    Owner,
    Admin,
    Operator,
    Viewer,
}

impl Role {
    pub fn permits(self, minimum: Role) -> bool {
        self.rank() >= minimum.rank()
    }

    fn rank(self) -> u8 {
        match self {
            Role::Viewer => 0,
            Role::Operator => 1,
            Role::Admin => 2,
            Role::Owner => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    pub password_hash: String,
    pub email_verified: bool,
    pub created_at: DateTime<Utc>,
}

impl User {
    pub fn new(
        email: impl Into<String>,
        display_name: impl Into<String>,
        password_hash: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            email: email.into(),
            display_name: display_name.into(),
            password_hash: password_hash.into(),
            email_verified: false,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserSession {
    pub id: Id,
    pub user_id: Id,
    pub token_hash: String,
    pub refresh_token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub refresh_expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl UserSession {
    pub fn new(
        user_id: Id,
        token_hash: impl Into<String>,
        refresh_token_hash: impl Into<String>,
        expires_at: DateTime<Utc>,
        refresh_expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            user_id,
            token_hash: token_hash.into(),
            refresh_token_hash: refresh_token_hash.into(),
            expires_at,
            refresh_expires_at,
            revoked_at: None,
            last_used_at: None,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKey {
    pub id: Id,
    pub org_id: Id,
    pub project_id: Option<Id>,
    pub name: String,
    pub key_hash: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_by: Option<Id>,
    pub created_at: DateTime<Utc>,
}

impl ApiKey {
    pub fn new(
        org_id: Id,
        project_id: Option<Id>,
        name: impl Into<String>,
        key_hash: impl Into<String>,
        scopes: Vec<String>,
        expires_at: Option<DateTime<Utc>>,
        created_by: Option<Id>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            org_id,
            project_id,
            name: name.into(),
            key_hash: key_hash.into(),
            scopes,
            expires_at,
            revoked_at: None,
            last_used_at: None,
            created_by,
            created_at: Utc::now(),
        }
    }

    pub fn has_scope(&self, required: &str) -> bool {
        self.scopes.iter().any(|scope| scope == required)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Org {
    pub id: Id,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}

impl Org {
    pub fn new(name: impl Into<String>, slug: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            name: name.into(),
            slug: slug.into(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Membership {
    pub id: Id,
    pub org_id: Id,
    pub user_id: Id,
    pub role: Role,
    pub created_at: DateTime<Utc>,
}

impl Membership {
    pub fn new(org_id: Id, user_id: Id, role: Role) -> Self {
        Self {
            id: Uuid::now_v7(),
            org_id,
            user_id,
            role,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}

impl Project {
    pub fn new(org_id: Id, name: impl Into<String>, slug: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            org_id,
            name: name.into(),
            slug: slug.into(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectFeature {
    pub project_id: Id,
    pub feature: String,
    pub enabled: bool,
    pub updated_by: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProjectFeature {
    pub fn new(
        project_id: Id,
        feature: impl Into<String>,
        enabled: bool,
        updated_by: Option<Id>,
    ) -> Self {
        let now = Utc::now();
        Self {
            project_id,
            feature: feature.into(),
            enabled,
            updated_by,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeviceStatus {
    Provisioned,
    Online,
    Offline,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Device {
    pub id: Id,
    pub project_id: Id,
    pub name: String,
    pub status: DeviceStatus,
    pub metadata: Value,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub latest_shadow: Value,
    pub created_at: DateTime<Utc>,
}

impl Device {
    pub fn new(project_id: Id, name: impl Into<String>, metadata: Value) -> Self {
        Self {
            id: Uuid::now_v7(),
            project_id,
            name: name.into(),
            status: DeviceStatus::Provisioned,
            metadata,
            last_seen_at: None,
            latest_shadow: Value::Object(JsonObject::new()),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CertificateStatus {
    Active,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceCertificate {
    pub id: Id,
    pub project_id: Id,
    pub device_id: Id,
    pub fingerprint_sha256: String,
    pub status: CertificateStatus,
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl DeviceCertificate {
    pub fn new(
        project_id: Id,
        device_id: Id,
        fingerprint_sha256: impl Into<String>,
        not_after: DateTime<Utc>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            project_id,
            device_id,
            fingerprint_sha256: fingerprint_sha256.into(),
            status: CertificateStatus::Active,
            not_before: now,
            not_after,
            created_at: now,
        }
    }

    pub fn revoke(&mut self) {
        self.status = CertificateStatus::Revoked;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StreamFieldType {
    String,
    Integer,
    Float,
    Boolean,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamField {
    pub name: String,
    pub field_type: StreamFieldType,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamDefinition {
    pub id: Id,
    pub project_id: Id,
    pub name: String,
    pub fields: Vec<StreamField>,
    pub created_at: DateTime<Utc>,
}

impl StreamDefinition {
    pub fn new(project_id: Id, name: impl Into<String>, fields: Vec<StreamField>) -> Self {
        Self {
            id: Uuid::now_v7(),
            project_id,
            name: name.into(),
            fields,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryPoint {
    pub project_id: Id,
    pub device_id: Id,
    pub stream: String,
    pub sequence: i64,
    pub ts: DateTime<Utc>,
    pub payload: Value,
    pub ingested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryAggregateBucket {
    pub project_id: Id,
    pub device_id: Option<Id>,
    pub stream: String,
    pub field: Option<String>,
    pub bucket_start: DateTime<Utc>,
    pub bucket_seconds: i64,
    pub count: i64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub avg: Option<f64>,
    pub last: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionType {
    pub id: Id,
    pub project_id: Id,
    pub name: String,
    pub schema: Value,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActionState {
    Queued,
    WaitingApproval,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub id: Id,
    pub project_id: Id,
    pub device_ids: Vec<Id>,
    pub name: String,
    pub payload: Value,
    pub state: ActionState,
    pub progress: u8,
    pub errors: Vec<String>,
    pub created_by: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Action {
    pub fn new(
        project_id: Id,
        device_ids: Vec<Id>,
        name: impl Into<String>,
        payload: Value,
        created_by: Option<Id>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            project_id,
            device_ids,
            name: name.into(),
            payload,
            state: ActionState::Queued,
            progress: 0,
            errors: Vec::new(),
            created_by,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionStatusUpdate {
    pub project_id: Id,
    pub action_id: Id,
    pub device_id: Id,
    pub state: ActionState,
    pub progress: u8,
    pub errors: Vec<String>,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionTargetTransition {
    pub project_id: Id,
    pub action_id: Id,
    pub device_ids: Option<Vec<Id>>,
    pub allowed_source_states: Vec<ActionState>,
    pub next_state: ActionState,
    pub progress: Option<u8>,
    pub errors: Option<Vec<String>>,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionTargetStatusChange {
    pub project_id: Id,
    pub action_id: Id,
    pub device_id: Id,
    pub state: ActionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionDispatchTarget {
    pub project_id: Id,
    pub action_id: Id,
    pub device_id: Id,
    pub name: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RemoteShellSessionState {
    Opening,
    Active,
    Closed,
    Expired,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteShellSession {
    pub id: Id,
    pub project_id: Id,
    pub device_id: Id,
    pub action_id: Option<Id>,
    pub state: RemoteShellSessionState,
    pub operator_token_hash: String,
    pub device_token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub opened_by: Option<Id>,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub close_reason: Option<String>,
    pub bytes_from_operator: i64,
    pub bytes_from_device: i64,
    pub last_activity_at: DateTime<Utc>,
}

impl RemoteShellSession {
    pub fn new(
        project_id: Id,
        device_id: Id,
        operator_token_hash: impl Into<String>,
        device_token_hash: impl Into<String>,
        expires_at: DateTime<Utc>,
        opened_by: Option<Id>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            project_id,
            device_id,
            action_id: None,
            state: RemoteShellSessionState::Opening,
            operator_token_hash: operator_token_hash.into(),
            device_token_hash: device_token_hash.into(),
            expires_at,
            opened_by,
            opened_at: now,
            closed_at: None,
            close_reason: None,
            bytes_from_operator: 0,
            bytes_from_device: 0,
            last_activity_at: now,
        }
    }

    pub fn is_open(&self, now: DateTime<Utc>) -> bool {
        self.closed_at.is_none()
            && self.expires_at > now
            && matches!(
                self.state,
                RemoteShellSessionState::Opening | RemoteShellSessionState::Active
            )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FirmwareArtifact {
    pub id: Id,
    pub project_id: Id,
    pub component: String,
    pub version: String,
    pub object_key: String,
    pub sha256: String,
    pub content_type: String,
    pub signature: Option<String>,
    pub size_bytes: i64,
    pub active: bool,
    pub uploaded_at: Option<DateTime<Utc>>,
    pub verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl FirmwareArtifact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: Id,
        component: impl Into<String>,
        version: impl Into<String>,
        object_key: impl Into<String>,
        sha256: impl Into<String>,
        content_type: impl Into<String>,
        signature: Option<String>,
        size_bytes: i64,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            project_id,
            component: component.into(),
            version: version.into(),
            object_key: object_key.into(),
            sha256: sha256.into(),
            content_type: content_type.into(),
            signature,
            size_bytes,
            active: true,
            uploaded_at: None,
            verified_at: None,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FirmwareRolloutState {
    Planned,
    WaitingApproval,
    Running,
    Completed,
    Failed,
    Cancelled,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FirmwareRollout {
    pub id: Id,
    pub project_id: Id,
    pub firmware_id: Id,
    pub action_id: Id,
    pub cohort_size: i64,
    pub strategy: String,
    pub rollback_strategy: Option<String>,
    pub state: FirmwareRolloutState,
    pub created_by: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewFirmwareRollout {
    pub project_id: Id,
    pub firmware_id: Id,
    pub action_id: Id,
    pub cohort_size: i64,
    pub strategy: String,
    pub rollback_strategy: Option<String>,
    pub state: FirmwareRolloutState,
    pub created_by: Option<Id>,
}

impl FirmwareRollout {
    pub fn new(input: NewFirmwareRollout) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            project_id: input.project_id,
            firmware_id: input.firmware_id,
            action_id: input.action_id,
            cohort_size: input.cohort_size,
            strategy: input.strategy,
            rollback_strategy: input.rollback_strategy,
            state: input.state,
            created_by: input.created_by,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AlertKind {
    Offline,
    Threshold,
    WindowAggregation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlertRule {
    pub id: Id,
    pub project_id: Id,
    pub name: String,
    pub kind: AlertKind,
    pub expression: Value,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AlertEventState {
    Firing,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlertEvent {
    pub id: Id,
    pub project_id: Id,
    pub alert_rule_id: Id,
    pub device_id: Option<Id>,
    pub dedupe_key: String,
    pub state: AlertEventState,
    pub message: String,
    pub observed_value: Option<f64>,
    pub threshold: Option<f64>,
    pub opened_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub last_seen_at: DateTime<Utc>,
    pub notification_attempts: i32,
    pub last_notification_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewAlertEvent {
    pub project_id: Id,
    pub alert_rule_id: Id,
    pub device_id: Option<Id>,
    pub dedupe_key: String,
    pub message: String,
    pub observed_value: Option<f64>,
    pub threshold: Option<f64>,
    pub ts: DateTime<Utc>,
}

impl AlertEvent {
    pub fn firing(input: NewAlertEvent) -> Self {
        Self {
            id: Uuid::now_v7(),
            project_id: input.project_id,
            alert_rule_id: input.alert_rule_id,
            device_id: input.device_id,
            dedupe_key: input.dedupe_key,
            state: AlertEventState::Firing,
            message: input.message,
            observed_value: input.observed_value,
            threshold: input.threshold,
            opened_at: input.ts,
            resolved_at: None,
            last_seen_at: input.ts,
            notification_attempts: 0,
            last_notification_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Dashboard {
    pub id: Id,
    pub project_id: Id,
    pub name: String,
    pub layout: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiagnosticsSessionState {
    Requested,
    UploadPending,
    Uploaded,
    Completed,
    Failed,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticsSession {
    pub id: Id,
    pub project_id: Id,
    pub device_id: Id,
    pub action_id: Option<Id>,
    pub object_key: String,
    pub state: DiagnosticsSessionState,
    pub upload_url_expires_at: Option<DateTime<Utc>>,
    pub download_url_expires_at: Option<DateTime<Utc>>,
    pub size_bytes: Option<i64>,
    pub sha256: Option<String>,
    pub error: Option<String>,
    pub created_by: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DiagnosticsSession {
    pub fn new(
        project_id: Id,
        device_id: Id,
        action_id: Option<Id>,
        object_key: impl Into<String>,
        created_by: Option<Id>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            project_id,
            device_id,
            action_id,
            object_key: object_key.into(),
            state: DiagnosticsSessionState::Requested,
            upload_url_expires_at: None,
            download_url_expires_at: None,
            size_bytes: None,
            sha256: None,
            error: None,
            created_by,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditLog {
    pub id: Id,
    pub org_id: Id,
    pub project_id: Option<Id>,
    pub actor_id: Option<Id>,
    pub action: String,
    pub resource: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

impl AuditLog {
    pub fn new(
        org_id: Id,
        project_id: Option<Id>,
        actor_id: Option<Id>,
        action: impl Into<String>,
        resource: impl Into<String>,
        metadata: Value,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            org_id,
            project_id,
            actor_id,
            action: action.into(),
            resource: resource.into(),
            metadata,
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_assign_ids_and_timestamps() {
        let org = Org::new("Acme Fleet", "acme");
        let project = Project::new(org.id, "Factory", "factory");
        let device = Device::new(project.id, "press-1", Value::Object(JsonObject::new()));

        assert_eq!(project.org_id, org.id);
        assert_eq!(device.project_id, project.id);
        assert_eq!(device.status, DeviceStatus::Provisioned);
        assert!(org.created_at <= Utc::now());
    }
}
