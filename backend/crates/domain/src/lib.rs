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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FirmwareArtifact {
    pub id: Id,
    pub project_id: Id,
    pub component: String,
    pub version: String,
    pub object_key: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

impl FirmwareArtifact {
    pub fn new(
        project_id: Id,
        component: impl Into<String>,
        version: impl Into<String>,
        object_key: impl Into<String>,
        sha256: impl Into<String>,
        size_bytes: i64,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            project_id,
            component: component.into(),
            version: version.into(),
            object_key: object_key.into(),
            sha256: sha256.into(),
            size_bytes,
            active: true,
            created_at: Utc::now(),
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Dashboard {
    pub id: Id,
    pub project_id: Id,
    pub name: String,
    pub layout: Value,
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
