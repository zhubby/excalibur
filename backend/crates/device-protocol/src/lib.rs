use chrono::{DateTime, TimeZone, Utc};
use excalibur_domain::Id;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("topic must start with v1")]
    UnsupportedVersion,
    #[error("topic has invalid shape")]
    InvalidTopic,
    #[error("topic contains invalid uuid")]
    InvalidUuid,
    #[error("topic project or device does not match authenticated device")]
    ScopeMismatch,
    #[error("payload must be a JSON array")]
    PayloadMustBeArray,
    #[error("payload item must be a JSON object")]
    PayloadItemMustBeObject,
    #[error("payload item is missing sequence")]
    MissingSequence,
    #[error("payload item is missing action_id")]
    MissingActionId,
    #[error("payload item has invalid action_id")]
    InvalidActionId,
    #[error("payload item has invalid timestamp")]
    InvalidTimestamp,
    #[error("payload is invalid: {0}")]
    InvalidPayload(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublishTopic {
    Telemetry {
        project_id: Id,
        device_id: Id,
        stream: String,
    },
    Shadow {
        project_id: Id,
        device_id: Id,
    },
    CommandStatus {
        project_id: Id,
        device_id: Id,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubscribeTopic {
    Commands { project_id: Id, device_id: Id },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryRecord {
    pub sequence: i64,
    pub timestamp: DateTime<Utc>,
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryIngestEnvelope {
    pub project_id: Id,
    pub device_id: Id,
    pub stream: String,
    pub points: Vec<TelemetryIngestPoint>,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryIngestPoint {
    pub sequence: i64,
    pub timestamp: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandStatusRecord {
    pub action_id: Id,
    pub state: String,
    pub progress: u8,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProvisioningMode {
    Csr,
    DevGeneratedKeypair,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceAgentAuthentication {
    pub ca_certificate: String,
    pub device_certificate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_private_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_private_key_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceAgentAuthConfig {
    pub broker: String,
    pub port: u16,
    pub project_id: Id,
    pub device_id: Id,
    pub certificate_id: Id,
    pub certificate_fingerprint_sha256: String,
    pub certificate_not_after: DateTime<Utc>,
    pub authentication: DeviceAgentAuthentication,
    pub provisioning_mode: ProvisioningMode,
    pub production: bool,
}

pub type DeviceConfig = DeviceAgentAuthConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceCommand {
    pub action_id: Id,
    pub name: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceCommandEnvelope {
    pub project_id: Id,
    pub device_id: Id,
    pub topic: String,
    pub command: DeviceCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OtaInstallPayload {
    pub firmware_id: Id,
    pub component: String,
    pub version: String,
    pub signed_url: String,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub size_bytes: i64,
}

impl OtaInstallPayload {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.component.trim().is_empty() {
            return Err(ProtocolError::InvalidPayload("component is required"));
        }
        if self.version.trim().is_empty() {
            return Err(ProtocolError::InvalidPayload("version is required"));
        }
        if !self.signed_url.starts_with("https://") && !self.signed_url.starts_with("http://") {
            return Err(ProtocolError::InvalidPayload("signed_url must be absolute"));
        }
        if self.sha256.len() != 64 || !self.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ProtocolError::InvalidPayload(
                "sha256 must be 64 hex characters",
            ));
        }
        if self.size_bytes <= 0 {
            return Err(ProtocolError::InvalidPayload("size_bytes must be positive"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticsCollectPayload {
    pub session_id: Id,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub include_logs: bool,
    #[serde(default)]
    pub include_system_stats: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteShellOpenPayload {
    pub session_id: Id,
    pub websocket_url: String,
    pub expires_at: DateTime<Utc>,
}

pub fn command_for_action(action_id: Id, name: impl Into<String>, payload: Value) -> DeviceCommand {
    DeviceCommand {
        action_id,
        name: name.into(),
        payload,
    }
}

pub fn telemetry_topic(project_id: Id, device_id: Id, stream: &str) -> String {
    format!("v1/p/{project_id}/d/{device_id}/telemetry/{stream}")
}

pub fn shadow_topic(project_id: Id, device_id: Id) -> String {
    format!("v1/p/{project_id}/d/{device_id}/shadow")
}

pub fn commands_topic(project_id: Id, device_id: Id) -> String {
    format!("v1/p/{project_id}/d/{device_id}/commands")
}

pub fn command_status_topic(project_id: Id, device_id: Id) -> String {
    format!("v1/p/{project_id}/d/{device_id}/commands/status")
}

pub fn parse_publish_topic(topic: &str) -> Result<PublishTopic, ProtocolError> {
    let parts: Vec<&str> = topic.trim_matches('/').split('/').collect();
    ensure_version(&parts)?;

    match parts.as_slice() {
        ["v1", "p", project, "d", device, "telemetry", stream] if !stream.is_empty() => {
            Ok(PublishTopic::Telemetry {
                project_id: parse_uuid(project)?,
                device_id: parse_uuid(device)?,
                stream: (*stream).to_owned(),
            })
        }
        ["v1", "p", project, "d", device, "shadow"] => Ok(PublishTopic::Shadow {
            project_id: parse_uuid(project)?,
            device_id: parse_uuid(device)?,
        }),
        ["v1", "p", project, "d", device, "commands", "status"] => {
            Ok(PublishTopic::CommandStatus {
                project_id: parse_uuid(project)?,
                device_id: parse_uuid(device)?,
            })
        }
        _ => Err(ProtocolError::InvalidTopic),
    }
}

pub fn parse_subscribe_topic(topic: &str) -> Result<SubscribeTopic, ProtocolError> {
    let parts: Vec<&str> = topic.trim_matches('/').split('/').collect();
    ensure_version(&parts)?;

    match parts.as_slice() {
        ["v1", "p", project, "d", device, "commands"] => Ok(SubscribeTopic::Commands {
            project_id: parse_uuid(project)?,
            device_id: parse_uuid(device)?,
        }),
        _ => Err(ProtocolError::InvalidTopic),
    }
}

pub fn validate_device_scope(
    topic_project_id: Id,
    topic_device_id: Id,
    project_id: Id,
    device_id: Id,
) -> Result<(), ProtocolError> {
    if topic_project_id == project_id && topic_device_id == device_id {
        Ok(())
    } else {
        Err(ProtocolError::ScopeMismatch)
    }
}

impl PublishTopic {
    pub fn project_id(&self) -> Id {
        match self {
            PublishTopic::Telemetry { project_id, .. }
            | PublishTopic::Shadow { project_id, .. }
            | PublishTopic::CommandStatus { project_id, .. } => *project_id,
        }
    }
}

pub fn decode_telemetry_payload(payload: Value) -> Result<Vec<TelemetryRecord>, ProtocolError> {
    let items = payload
        .as_array()
        .ok_or(ProtocolError::PayloadMustBeArray)?;
    items
        .iter()
        .map(|item| {
            let object = item
                .as_object()
                .ok_or(ProtocolError::PayloadItemMustBeObject)?;
            let sequence = object
                .get("sequence")
                .and_then(Value::as_i64)
                .ok_or(ProtocolError::MissingSequence)?;
            let timestamp = object
                .get("timestamp")
                .ok_or(ProtocolError::InvalidTimestamp)
                .and_then(parse_timestamp)?;
            let mut fields = object.clone();
            fields.remove("sequence");
            fields.remove("timestamp");
            Ok(TelemetryRecord {
                sequence,
                timestamp,
                fields,
            })
        })
        .collect()
}

pub fn decode_command_status_payload(
    payload: Value,
) -> Result<Vec<CommandStatusRecord>, ProtocolError> {
    let items = payload
        .as_array()
        .ok_or(ProtocolError::PayloadMustBeArray)?;
    items
        .iter()
        .map(|item| {
            let object = item
                .as_object()
                .ok_or(ProtocolError::PayloadItemMustBeObject)?;
            let action_id = object
                .get("action_id")
                .and_then(Value::as_str)
                .ok_or(ProtocolError::MissingActionId)
                .and_then(|value| {
                    Uuid::parse_str(value).map_err(|_| ProtocolError::InvalidActionId)
                })?;
            let state = object
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("Running")
                .to_owned();
            let progress = object
                .get("progress")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(100) as u8;
            let errors = object
                .get("errors")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_else(Vec::new);

            Ok(CommandStatusRecord {
                action_id,
                state,
                progress,
                errors,
            })
        })
        .collect()
}

fn ensure_version(parts: &[&str]) -> Result<(), ProtocolError> {
    match parts.first() {
        Some(&"v1") => Ok(()),
        Some(_) => Err(ProtocolError::UnsupportedVersion),
        None => Err(ProtocolError::InvalidTopic),
    }
}

fn parse_uuid(input: &str) -> Result<Uuid, ProtocolError> {
    Uuid::parse_str(input).map_err(|_| ProtocolError::InvalidUuid)
}

fn parse_timestamp(value: &Value) -> Result<DateTime<Utc>, ProtocolError> {
    if let Some(ms) = value.as_i64() {
        return Utc
            .timestamp_millis_opt(ms)
            .single()
            .ok_or(ProtocolError::InvalidTimestamp);
    }

    if let Some(text) = value.as_str() {
        return DateTime::parse_from_rfc3339(text)
            .map(|ts| ts.with_timezone(&Utc))
            .map_err(|_| ProtocolError::InvalidTimestamp);
    }

    Err(ProtocolError::InvalidTimestamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_valid_publish_topics() {
        let project_id = Uuid::now_v7();
        let device_id = Uuid::now_v7();
        let topic = telemetry_topic(project_id, device_id, "temperature");

        assert_eq!(
            parse_publish_topic(&topic).unwrap(),
            PublishTopic::Telemetry {
                project_id,
                device_id,
                stream: "temperature".to_owned()
            }
        );
        assert_eq!(
            parse_publish_topic(&shadow_topic(project_id, device_id)).unwrap(),
            PublishTopic::Shadow {
                project_id,
                device_id
            }
        );
        assert_eq!(
            parse_publish_topic(&command_status_topic(project_id, device_id)).unwrap(),
            PublishTopic::CommandStatus {
                project_id,
                device_id
            }
        );
    }

    #[test]
    fn rejects_cross_scope_publish() {
        let project_id = Uuid::now_v7();
        let device_id = Uuid::now_v7();
        let other_project = Uuid::now_v7();

        assert_eq!(
            validate_device_scope(other_project, device_id, project_id, device_id),
            Err(ProtocolError::ScopeMismatch)
        );
    }

    #[test]
    fn decodes_batched_telemetry_payload() {
        let payload = json!([
            {
                "sequence": 1,
                "timestamp": 1710760059006i64,
                "temperature": 24.5,
                "status": "ok"
            }
        ]);

        let records = decode_telemetry_payload(payload).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].sequence, 1);
        assert_eq!(records[0].fields["status"], "ok");
        assert!(records[0].fields.get("timestamp").is_none());
    }

    #[test]
    fn decodes_command_status_payload() {
        let action_id = Uuid::now_v7();
        let records = decode_command_status_payload(json!([
            {
                "action_id": action_id.to_string(),
                "state": "completed",
                "progress": 105,
                "errors": ["ignored warning"]
            }
        ]))
        .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].action_id, action_id);
        assert_eq!(records[0].state, "completed");
        assert_eq!(records[0].progress, 100);
        assert_eq!(records[0].errors, vec!["ignored warning"]);
    }

    #[test]
    fn device_agent_auth_json_roundtrips_with_key_path() {
        let config = DeviceAgentAuthConfig {
            broker: "mqtt.excalibur.local".to_owned(),
            port: 8883,
            project_id: Uuid::now_v7(),
            device_id: Uuid::now_v7(),
            certificate_id: Uuid::now_v7(),
            certificate_fingerprint_sha256: "a".repeat(64),
            certificate_not_after: Utc::now(),
            authentication: DeviceAgentAuthentication {
                ca_certificate: "ca".to_owned(),
                device_certificate: "cert".to_owned(),
                device_private_key: None,
                device_private_key_path: Some("/etc/excalibur/device.key".to_owned()),
            },
            provisioning_mode: ProvisioningMode::Csr,
            production: true,
        };

        let roundtrip: DeviceAgentAuthConfig =
            serde_json::from_value(serde_json::to_value(&config).unwrap()).unwrap();

        assert_eq!(roundtrip, config);
        assert_eq!(
            roundtrip.authentication.device_private_key_path.as_deref(),
            Some("/etc/excalibur/device.key")
        );
    }

    #[test]
    fn command_payload_keeps_json_value_shape() {
        let action_id = Uuid::now_v7();
        let command = command_for_action(
            action_id,
            "ota.install",
            json!({
                "firmware_id": Uuid::now_v7(),
                "component": "main"
            }),
        );
        let value = serde_json::to_value(&command).unwrap();

        assert_eq!(value["action_id"], action_id.to_string());
        assert_eq!(value["name"], "ota.install");
        assert!(value["payload"].is_object());
        assert!(!value["payload"].is_string());
    }

    #[test]
    fn validates_ota_install_payload() {
        let payload = OtaInstallPayload {
            firmware_id: Uuid::now_v7(),
            component: "main".to_owned(),
            version: "1.2.3".to_owned(),
            signed_url: "https://objects.example/firmware.bin?sig=1".to_owned(),
            sha256: "a".repeat(64),
            signature: Some("ed25519:test".to_owned()),
            size_bytes: 1024,
        };

        assert!(payload.validate().is_ok());

        let invalid = OtaInstallPayload {
            sha256: "not-a-sha".to_owned(),
            ..payload
        };
        assert_eq!(
            invalid.validate(),
            Err(ProtocolError::InvalidPayload(
                "sha256 must be 64 hex characters"
            ))
        );
    }
}
