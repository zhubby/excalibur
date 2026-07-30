use excalibur_domain::{
    Action, ActionState, AlertRule, ApiKey, AuditLog, CertificateStatus, Dashboard, Device,
    DeviceCertificate, DeviceStatus, FirmwareArtifact, Id, Membership, Org, Project, Role,
    StreamDefinition, TelemetryPoint, User, UserSession,
};
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};

use crate::{
    StoreError, StoreResult,
    postgres::helpers::{map_decode_error, map_json_error},
};

pub(super) fn role_to_db(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Operator => "operator",
        Role::Viewer => "viewer",
    }
}

pub(super) fn role_from_db(value: &str) -> StoreResult<Role> {
    match value {
        "owner" => Ok(Role::Owner),
        "admin" => Ok(Role::Admin),
        "operator" => Ok(Role::Operator),
        "viewer" => Ok(Role::Viewer),
        _ => Err(StoreError::Database(format!(
            "unknown member role: {value}"
        ))),
    }
}

pub(super) fn device_status_to_db(status: &DeviceStatus) -> &'static str {
    match status {
        DeviceStatus::Provisioned => "provisioned",
        DeviceStatus::Online => "online",
        DeviceStatus::Offline => "offline",
        DeviceStatus::Disabled => "disabled",
    }
}

pub(super) fn device_status_from_db(value: &str) -> StoreResult<DeviceStatus> {
    match value {
        "provisioned" => Ok(DeviceStatus::Provisioned),
        "online" => Ok(DeviceStatus::Online),
        "offline" => Ok(DeviceStatus::Offline),
        "disabled" => Ok(DeviceStatus::Disabled),
        _ => Err(StoreError::Database(format!(
            "unknown device status: {value}"
        ))),
    }
}

pub(super) fn certificate_status_to_db(status: &CertificateStatus) -> &'static str {
    match status {
        CertificateStatus::Active => "active",
        CertificateStatus::Revoked => "revoked",
        CertificateStatus::Expired => "expired",
    }
}

pub(super) fn certificate_status_from_db(value: &str) -> StoreResult<CertificateStatus> {
    match value {
        "active" => Ok(CertificateStatus::Active),
        "revoked" => Ok(CertificateStatus::Revoked),
        "expired" => Ok(CertificateStatus::Expired),
        _ => Err(StoreError::Database(format!(
            "unknown certificate status: {value}"
        ))),
    }
}

pub(super) fn action_state_to_db(state: &ActionState) -> &'static str {
    match state {
        ActionState::Queued => "queued",
        ActionState::WaitingApproval => "waiting_approval",
        ActionState::Running => "running",
        ActionState::Completed => "completed",
        ActionState::Failed => "failed",
        ActionState::Cancelled => "cancelled",
        ActionState::TimedOut => "timed_out",
    }
}

pub(super) fn action_state_from_db(value: &str) -> StoreResult<ActionState> {
    match value {
        "queued" => Ok(ActionState::Queued),
        "waiting_approval" => Ok(ActionState::WaitingApproval),
        "running" => Ok(ActionState::Running),
        "completed" => Ok(ActionState::Completed),
        "failed" => Ok(ActionState::Failed),
        "cancelled" => Ok(ActionState::Cancelled),
        "timed_out" => Ok(ActionState::TimedOut),
        _ => Err(StoreError::Database(format!(
            "unknown action state: {value}"
        ))),
    }
}

pub(super) fn alert_kind_to_db(kind: &excalibur_domain::AlertKind) -> &'static str {
    match kind {
        excalibur_domain::AlertKind::Offline => "offline",
        excalibur_domain::AlertKind::Threshold => "threshold",
        excalibur_domain::AlertKind::WindowAggregation => "window_aggregation",
    }
}

pub(super) fn alert_kind_from_db(value: &str) -> StoreResult<excalibur_domain::AlertKind> {
    match value {
        "offline" => Ok(excalibur_domain::AlertKind::Offline),
        "threshold" => Ok(excalibur_domain::AlertKind::Threshold),
        "window_aggregation" => Ok(excalibur_domain::AlertKind::WindowAggregation),
        _ => Err(StoreError::Database(format!("unknown alert kind: {value}"))),
    }
}

pub(super) fn map_user(row: &PgRow) -> StoreResult<User> {
    Ok(User {
        id: row.try_get("id").map_err(map_decode_error)?,
        email: row.try_get("email").map_err(map_decode_error)?,
        display_name: row.try_get("display_name").map_err(map_decode_error)?,
        password_hash: row.try_get("password_hash").map_err(map_decode_error)?,
        email_verified: row.try_get("email_verified").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_user_session(row: &PgRow) -> StoreResult<UserSession> {
    Ok(UserSession {
        id: row.try_get("id").map_err(map_decode_error)?,
        user_id: row.try_get("user_id").map_err(map_decode_error)?,
        token_hash: row.try_get("token_hash").map_err(map_decode_error)?,
        refresh_token_hash: row
            .try_get("refresh_token_hash")
            .map_err(map_decode_error)?,
        expires_at: row.try_get("expires_at").map_err(map_decode_error)?,
        refresh_expires_at: row
            .try_get("refresh_expires_at")
            .map_err(map_decode_error)?,
        revoked_at: row.try_get("revoked_at").map_err(map_decode_error)?,
        last_used_at: row.try_get("last_used_at").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_api_key(row: &PgRow) -> StoreResult<ApiKey> {
    Ok(ApiKey {
        id: row.try_get("id").map_err(map_decode_error)?,
        org_id: row.try_get("org_id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        key_hash: row.try_get("key_hash").map_err(map_decode_error)?,
        scopes: row.try_get("scopes").map_err(map_decode_error)?,
        expires_at: row.try_get("expires_at").map_err(map_decode_error)?,
        revoked_at: row.try_get("revoked_at").map_err(map_decode_error)?,
        last_used_at: row.try_get("last_used_at").map_err(map_decode_error)?,
        created_by: row.try_get("created_by").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_org(row: &PgRow) -> StoreResult<Org> {
    Ok(Org {
        id: row.try_get("id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        slug: row.try_get("slug").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_membership(row: &PgRow) -> StoreResult<Membership> {
    let role: String = row.try_get("role").map_err(map_decode_error)?;
    Ok(Membership {
        id: row.try_get("id").map_err(map_decode_error)?,
        org_id: row.try_get("org_id").map_err(map_decode_error)?,
        user_id: row.try_get("user_id").map_err(map_decode_error)?,
        role: role_from_db(&role)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_project(row: &PgRow) -> StoreResult<Project> {
    Ok(Project {
        id: row.try_get("id").map_err(map_decode_error)?,
        org_id: row.try_get("org_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        slug: row.try_get("slug").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_device(row: &PgRow) -> StoreResult<Device> {
    let status: String = row.try_get("status").map_err(map_decode_error)?;
    Ok(Device {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        status: device_status_from_db(&status)?,
        metadata: row.try_get("metadata").map_err(map_decode_error)?,
        latest_shadow: row.try_get("latest_shadow").map_err(map_decode_error)?,
        last_seen_at: row.try_get("last_seen_at").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_certificate(row: &PgRow) -> StoreResult<DeviceCertificate> {
    let status: String = row.try_get("status").map_err(map_decode_error)?;
    Ok(DeviceCertificate {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        device_id: row.try_get("device_id").map_err(map_decode_error)?,
        fingerprint_sha256: row
            .try_get("fingerprint_sha256")
            .map_err(map_decode_error)?,
        status: certificate_status_from_db(&status)?,
        not_before: row.try_get("not_before").map_err(map_decode_error)?,
        not_after: row.try_get("not_after").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_stream(row: &PgRow) -> StoreResult<StreamDefinition> {
    let fields: Value = row.try_get("fields").map_err(map_decode_error)?;
    Ok(StreamDefinition {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        fields: serde_json::from_value(fields).map_err(map_json_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_telemetry(row: &PgRow) -> StoreResult<TelemetryPoint> {
    Ok(TelemetryPoint {
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        device_id: row.try_get("device_id").map_err(map_decode_error)?,
        stream: row.try_get("stream").map_err(map_decode_error)?,
        sequence: row.try_get("sequence").map_err(map_decode_error)?,
        ts: row.try_get("ts").map_err(map_decode_error)?,
        payload: row.try_get("payload").map_err(map_decode_error)?,
        ingested_at: row.try_get("ingested_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_action_row(row: &PgRow, device_ids: Vec<Id>) -> StoreResult<Action> {
    let state: String = row.try_get("state").map_err(map_decode_error)?;
    let progress: i16 = row.try_get("progress").map_err(map_decode_error)?;
    Ok(Action {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        device_ids,
        name: row.try_get("name").map_err(map_decode_error)?,
        payload: row.try_get("payload").map_err(map_decode_error)?,
        state: action_state_from_db(&state)?,
        progress: progress.clamp(0, 100) as u8,
        errors: row.try_get("errors").map_err(map_decode_error)?,
        created_by: row.try_get("created_by").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
        updated_at: row.try_get("updated_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_firmware(row: &PgRow) -> StoreResult<FirmwareArtifact> {
    Ok(FirmwareArtifact {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        component: row.try_get("component").map_err(map_decode_error)?,
        version: row.try_get("version").map_err(map_decode_error)?,
        object_key: row.try_get("object_key").map_err(map_decode_error)?,
        sha256: row.try_get("sha256").map_err(map_decode_error)?,
        size_bytes: row.try_get("size_bytes").map_err(map_decode_error)?,
        active: row.try_get("active").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

pub(super) fn map_alert(row: &PgRow) -> StoreResult<AlertRule> {
    let kind: String = row.try_get("kind").map_err(map_decode_error)?;
    Ok(AlertRule {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        kind: alert_kind_from_db(&kind)?,
        expression: row.try_get("expression").map_err(map_decode_error)?,
        enabled: row.try_get("enabled").map_err(map_decode_error)?,
    })
}

pub(super) fn map_dashboard(row: &PgRow) -> StoreResult<Dashboard> {
    Ok(Dashboard {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        layout: row.try_get("layout").map_err(map_decode_error)?,
    })
}

pub(super) fn map_audit(row: &PgRow) -> StoreResult<AuditLog> {
    Ok(AuditLog {
        id: row.try_get("id").map_err(map_decode_error)?,
        org_id: row.try_get("org_id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        actor_id: row.try_get("actor_id").map_err(map_decode_error)?,
        action: row.try_get("action").map_err(map_decode_error)?,
        resource: row.try_get("resource").map_err(map_decode_error)?,
        metadata: row.try_get("metadata").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}
