//! ESP SDK for the Excalibur native device protocol.
//!
//! The protocol-facing helpers in this crate are available without ESP-IDF so
//! topic and payload contracts can be tested on a host machine. The MQTT client
//! is enabled by the default `esp-idf` feature.

use core::fmt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map, Value};

#[cfg(feature = "esp-idf")]
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
    fs, ptr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

#[cfg(feature = "esp-idf")]
use anyhow::{bail, Error};
#[cfg(feature = "esp-idf")]
use embedded_svc::{
    mqtt::client::{Connection, Details, Event, Message, MessageImpl, QoS},
    utils::mqtt::client::ConnState,
};
#[cfg(feature = "esp-idf")]
use esp_idf_svc::{
    mqtt::client::{EspMqttClient, MqttClientConfiguration},
    systime::EspSystemTime,
    tls::X509,
};
#[cfg(feature = "esp-idf")]
use esp_idf_sys::{
    esp_err_to_name, esp_http_client_cleanup, esp_http_client_close, esp_http_client_config_t,
    esp_http_client_fetch_headers, esp_http_client_init, esp_http_client_open,
    esp_http_client_read, esp_ota_begin, esp_ota_end, esp_ota_get_next_update_partition,
    esp_ota_handle_t, esp_ota_set_boot_partition, esp_ota_write, esp_restart,
    esp_vfs_spiffs_conf_t, esp_vfs_spiffs_register, esp_vfs_unregister, EspError, ESP_OK,
    OTA_SIZE_UNKNOWN,
};
#[cfg(feature = "esp-idf")]
use log::{error, info};

#[cfg(feature = "esp-idf")]
type CommandHandler = &'static (dyn Fn(Command, &ExcaliburClient) + Send + Sync);

#[cfg(feature = "esp-idf")]
const CONFIG_PATH: &str = "/spiffs/device_config.json";

/// Device configuration returned by Excalibur provisioning APIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceConfig {
    pub project_id: String,
    pub broker: String,
    pub port: u16,
    pub device_id: String,
    pub authentication: Authentication,
    #[serde(default)]
    pub provisioning_mode: Option<ProvisioningMode>,
    #[serde(default)]
    pub production: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Authentication {
    pub ca_certificate: String,
    pub device_certificate: String,
    #[serde(default)]
    pub device_private_key: Option<String>,
    #[serde(default)]
    pub device_private_key_path: Option<String>,
}

impl Authentication {
    pub fn private_key_pem(&self) -> anyhow::Result<String> {
        if let Some(key) = &self.device_private_key {
            return Ok(key.clone());
        }
        if let Some(path) = &self.device_private_key_path {
            return Ok(std::fs::read_to_string(path)?);
        }
        anyhow::bail!("authentication.device_private_key or device_private_key_path is required")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProvisioningMode {
    Csr,
    DevGeneratedKeypair,
}

/// Command sent by Excalibur over the device commands topic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Command {
    pub action_id: String,
    pub name: String,
    #[serde(default)]
    pub payload: Value,
}

impl Command {
    pub fn payload_as<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.payload.clone())
    }

    pub fn payload_string(&self) -> String {
        match &self.payload {
            Value::String(value) => value.clone(),
            value => value.to_string(),
        }
    }
}

/// Wire states accepted by Excalibur command status ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandState {
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl CommandState {
    pub fn as_str(self) -> &'static str {
        match self {
            CommandState::Running => "Running",
            CommandState::Completed => "Completed",
            CommandState::Failed => "Failed",
            CommandState::Cancelled => "Cancelled",
            CommandState::TimedOut => "TimedOut",
        }
    }
}

impl fmt::Display for CommandState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OtaInstallPayload {
    pub firmware_id: String,
    pub component: String,
    pub version: String,
    pub signed_url: String,
    pub sha256: String,
    #[serde(default)]
    pub signature: Option<String>,
    pub size_bytes: i64,
}

impl OtaInstallPayload {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.component.trim().is_empty() {
            anyhow::bail!("component is required");
        }
        if self.version.trim().is_empty() {
            anyhow::bail!("version is required");
        }
        if !self.signed_url.starts_with("https://") && !self.signed_url.starts_with("http://") {
            anyhow::bail!("signed_url must be absolute");
        }
        if self.sha256.len() != 64 || !self.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!("sha256 must be 64 hex characters");
        }
        if self.size_bytes <= 0 {
            anyhow::bail!("size_bytes must be positive");
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct CommandStatus<'a> {
    action_id: &'a str,
    state: &'static str,
    progress: u8,
    errors: Vec<String>,
}

pub fn telemetry_topic(project_id: &str, device_id: &str, stream: &str) -> String {
    format!("v1/p/{project_id}/d/{device_id}/telemetry/{stream}")
}

pub fn shadow_topic(project_id: &str, device_id: &str) -> String {
    format!("v1/p/{project_id}/d/{device_id}/shadow")
}

pub fn commands_topic(project_id: &str, device_id: &str) -> String {
    format!("v1/p/{project_id}/d/{device_id}/commands")
}

pub fn command_status_topic(project_id: &str, device_id: &str) -> String {
    format!("v1/p/{project_id}/d/{device_id}/commands/status")
}

pub fn telemetry_payload(
    sequence: i64,
    timestamp_millis: u64,
    payload: impl Serialize,
) -> anyhow::Result<Vec<u8>> {
    let mut record = serialize_object_fields(payload)?;
    record.insert("sequence".to_owned(), Value::from(sequence));
    record.insert("timestamp".to_owned(), Value::from(timestamp_millis));
    Ok(serde_json::to_vec(&[Value::Object(record)])?)
}

pub fn shadow_payload(payload: impl Serialize) -> anyhow::Result<Vec<u8>> {
    match serde_json::to_value(payload)? {
        Value::Object(object) => Ok(serde_json::to_vec(&Value::Object(object))?),
        _ => anyhow::bail!("shadow payload must be a JSON object"),
    }
}

pub fn command_status_payload(
    action_id: &str,
    state: CommandState,
    progress: u8,
    errors: &[impl AsRef<str>],
) -> anyhow::Result<Vec<u8>> {
    let status = CommandStatus {
        action_id,
        state: state.as_str(),
        progress: progress.min(100),
        errors: errors
            .iter()
            .map(|error| error.as_ref().to_owned())
            .collect(),
    };
    Ok(serde_json::to_vec(&[status])?)
}

fn serialize_object_fields(payload: impl Serialize) -> anyhow::Result<Map<String, Value>> {
    match serde_json::to_value(payload)? {
        Value::Object(object) => Ok(object),
        value => {
            let mut object = Map::new();
            object.insert("value".to_owned(), value);
            Ok(object)
        }
    }
}

#[cfg(feature = "esp-idf")]
/// Client connected to an Excalibur MQTT broker.
pub struct ExcaliburClient {
    mqtt_client: Mutex<EspMqttClient<ConnState<MessageImpl, EspError>>>,
    command_handlers: Mutex<BTreeMap<String, CommandHandler>>,
    pub device_id: String,
    pub project_id: String,
    ca_cert: &'static CStr,
    device_cert: &'static CStr,
    device_key: &'static CStr,
}

#[cfg(feature = "esp-idf")]
impl ExcaliburClient {
    /// Read `/spiffs/device_config.json`, connect to Excalibur, and subscribe to commands.
    pub fn init() -> anyhow::Result<Arc<Self>> {
        let config = read_spiffs_config(CONFIG_PATH)?;
        Self::from_config(config)
    }

    pub fn from_config(device_config: DeviceConfig) -> anyhow::Result<Arc<Self>> {
        let private_key = device_config.authentication.private_key_pem()?;
        let ca_cert = Box::leak(
            CString::new(device_config.authentication.ca_certificate)?.into_boxed_c_str(),
        );
        let device_cert = Box::leak(
            CString::new(device_config.authentication.device_certificate)?.into_boxed_c_str(),
        );
        let device_key = Box::leak(CString::new(private_key)?.into_boxed_c_str());

        let mqtt_config = MqttClientConfiguration {
            server_certificate: Some(X509::pem(ca_cert)),
            client_certificate: Some(X509::pem(device_cert)),
            private_key: Some(X509::pem(device_key)),
            ..Default::default()
        };

        let broker_uri = format!("mqtts://{}:{}", device_config.broker, device_config.port);
        let commands_topic = commands_topic(&device_config.project_id, &device_config.device_id);
        let (mqtt_client, mut connection) = EspMqttClient::new_with_conn(broker_uri, &mqtt_config)?;

        let excalibur_client = Arc::new(ExcaliburClient {
            command_handlers: Mutex::new(BTreeMap::new()),
            mqtt_client: Mutex::new(mqtt_client),
            device_id: device_config.device_id,
            project_id: device_config.project_id,
            ca_cert,
            device_cert,
            device_key,
        });

        let (tx, rx) = std::sync::mpsc::channel::<Command>();
        let cloned_client = excalibur_client.clone();
        thread::spawn(move || {
            info!("MQTT listening for Excalibur commands");
            while let Some(message_event) = connection.next() {
                match message_event {
                    Ok(Event::Received(data)) => {
                        if data.details() == &Details::Complete {
                            match serde_json::from_slice::<Command>(data.data()) {
                                Ok(command) => {
                                    if tx.send(command).is_err() {
                                        error!("failed to enqueue command")
                                    }
                                }
                                Err(error) => error!("failed to decode command: {error}"),
                            };
                        }
                    }
                    Ok(Event::Connected(_)) => {
                        if cloned_client
                            .mqtt_client
                            .lock()
                            .unwrap()
                            .subscribe(&commands_topic, QoS::AtLeastOnce)
                            .is_ok()
                        {
                            info!("subscribed to Excalibur commands");
                        }
                    }
                    _ => info!("MQTT event: {message_event:?}"),
                };
            }

            error!("MQTT connection loop exited");
        });

        let cloned_client = excalibur_client.clone();
        thread::spawn(move || -> anyhow::Result<()> {
            loop {
                let command = rx.recv()?;
                if let Some(command_fn) = cloned_client
                    .command_handlers
                    .lock()
                    .unwrap()
                    .get(&command.name)
                {
                    command_fn(command, &cloned_client)
                } else {
                    error!("command handler does not exist for {}", command.name);
                    cloned_client
                        .publish_command_status(
                            &command.action_id,
                            CommandState::Failed,
                            0,
                            &["Unregistered command"],
                        )
                        .ok();
                }
            }
        });

        Ok(excalibur_client)
    }

    pub fn register_command_handler(&self, command_name: String, command_function: CommandHandler) {
        info!("setting command handler for {command_name}");
        self.command_handlers
            .lock()
            .unwrap()
            .insert(command_name, command_function);
    }

    pub fn publish_telemetry(
        &self,
        stream_name: &str,
        sequence: i64,
        payload: impl Serialize,
    ) -> anyhow::Result<u32> {
        let publish_topic = telemetry_topic(&self.project_id, &self.device_id, stream_name);
        let timestamp = EspSystemTime {}.now().as_millis() as u64;
        let payload = telemetry_payload(sequence, timestamp, payload)?;
        self.mqtt_client
            .lock()
            .unwrap()
            .publish(&publish_topic, QoS::AtLeastOnce, false, &payload)
            .map_err(Error::msg)
    }

    pub fn publish_shadow(&self, payload: impl Serialize) -> anyhow::Result<u32> {
        let publish_topic = shadow_topic(&self.project_id, &self.device_id);
        let payload = shadow_payload(payload)?;
        self.mqtt_client
            .lock()
            .unwrap()
            .publish(&publish_topic, QoS::AtLeastOnce, false, &payload)
            .map_err(Error::msg)
    }

    pub fn publish_command_status(
        &self,
        action_id: &str,
        state: CommandState,
        progress: u8,
        errors: &[impl AsRef<str>],
    ) -> anyhow::Result<u32> {
        let publish_topic = command_status_topic(&self.project_id, &self.device_id);
        let payload = command_status_payload(action_id, state, progress, errors)?;
        self.mqtt_client
            .lock()
            .unwrap()
            .publish(&publish_topic, QoS::AtLeastOnce, false, &payload)
            .map_err(Error::msg)
    }

    /// Register the built-in `ota.install` command handler.
    pub fn enable_ota(&self) {
        self.register_command_handler("ota.install".into(), &handle_ota);
    }
}

#[cfg(feature = "esp-idf")]
fn read_spiffs_config(path: &str) -> anyhow::Result<DeviceConfig> {
    let base_path: CString = CString::new("/spiffs").unwrap();
    let configuration_spiffs = esp_vfs_spiffs_conf_t {
        base_path: base_path.as_ptr(),
        format_if_mount_failed: true,
        max_files: 5,
        partition_label: ptr::null(),
    };

    unsafe {
        let ret = esp_vfs_spiffs_register(&configuration_spiffs);
        if ret != ESP_OK {
            esp_vfs_unregister(configuration_spiffs.base_path);
            bail!(
                "failed to mount SPIFFS: {:?}",
                CStr::from_ptr(esp_err_to_name(ret))
            );
        }
    }

    let config = fs::read_to_string(path).and_then(|config| {
        let mut device_config: DeviceConfig = serde_json::from_str(&config)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        if device_config.authentication.device_private_key.is_none() {
            if let Some(key_path) = &device_config.authentication.device_private_key_path {
                device_config.authentication.device_private_key =
                    Some(fs::read_to_string(key_path)?);
            }
        }
        Ok(device_config)
    });

    unsafe {
        esp_vfs_unregister(configuration_spiffs.base_path);
    }

    Ok(config?)
}

#[cfg(feature = "esp-idf")]
fn handle_ota(command: Command, excalibur_client: &ExcaliburClient) {
    let ota = match command.payload_as::<OtaInstallPayload>() {
        Ok(payload) => payload,
        Err(error) => {
            error!("failed to deserialize OTA payload: {error}");
            excalibur_client
                .publish_command_status(
                    &command.action_id,
                    CommandState::Failed,
                    0,
                    &[format!("Invalid OTA payload: {error}")],
                )
                .ok();
            return;
        }
    };

    if let Err(error) = ota.validate() {
        error!("invalid OTA payload: {error}");
        excalibur_client
            .publish_command_status(
                &command.action_id,
                CommandState::Failed,
                0,
                &[format!("Invalid OTA payload: {error}")],
            )
            .ok();
        return;
    }

    info!(
        "installing {} firmware version {}",
        ota.component, ota.version
    );
    let url = match CString::new(ota.signed_url) {
        Ok(url) => url,
        Err(error) => {
            excalibur_client
                .publish_command_status(
                    &command.action_id,
                    CommandState::Failed,
                    0,
                    &[format!("Invalid OTA URL: {error}")],
                )
                .ok();
            return;
        }
    };
    let mut buf = [0; 512];

    let http_config: esp_http_client_config_t = esp_http_client_config_t {
        url: url.as_ptr(),
        cert_pem: excalibur_client.ca_cert.as_ptr(),
        client_cert_pem: excalibur_client.device_cert.as_ptr(),
        client_key_pem: excalibur_client.device_key.as_ptr(),
        ..Default::default()
    };

    unsafe {
        info!("initializing OTA HTTP client");
        let client = esp_http_client_init(&http_config);

        if esp_http_client_open(client, 0) != ESP_OK {
            error!("failed to open OTA connection");
            esp_http_client_cleanup(client);
            excalibur_client
                .publish_command_status(
                    &command.action_id,
                    CommandState::Failed,
                    0,
                    &["Failed to open OTA connection"],
                )
                .ok();
            return;
        }

        let partition = esp_ota_get_next_update_partition(ptr::null());
        let mut ota_handle: esp_ota_handle_t = 0;

        let ret = esp_ota_begin(partition, OTA_SIZE_UNKNOWN as usize, &mut ota_handle);
        if ret != ESP_OK {
            error!("failed to begin OTA: {ret}");
            esp_http_client_cleanup(client);
            return;
        }

        let content_length = esp_http_client_fetch_headers(client);
        let mut total_read = 0;
        let mut next_progress = 10;
        while total_read < content_length {
            let len_read = esp_http_client_read(client, buf.as_mut_ptr() as _, buf.len() as _);
            if len_read < 0 {
                error!("failed to read OTA data");
                esp_http_client_close(client);
                esp_http_client_cleanup(client);
                return;
            }
            let ret = esp_ota_write(ota_handle, buf.as_ptr() as _, len_read as usize);
            if ret != ESP_OK {
                error!("failed to write OTA data: {ret}");
                esp_http_client_close(client);
                esp_http_client_cleanup(client);
                return;
            }
            total_read += len_read;
            let percentage = ((total_read as f32 / content_length as f32) * 100.0) as u8;
            if percentage >= next_progress || percentage == 100 {
                let state = if percentage == 100 {
                    CommandState::Completed
                } else {
                    CommandState::Running
                };
                excalibur_client
                    .publish_command_status(&command.action_id, state, percentage, &[] as &[&str])
                    .ok();
                next_progress = next_progress.saturating_add(10);
            }
            buf.fill(0);
            thread::sleep(Duration::from_millis(200));
        }

        esp_http_client_close(client);
        esp_http_client_cleanup(client);
        let ret = esp_ota_end(ota_handle);
        if ret != ESP_OK {
            error!("failed to end OTA: {ret}");
            return;
        }
        let ret = esp_ota_set_boot_partition(partition);
        if ret != ESP_OK {
            error!("failed to set OTA boot partition: {ret}");
            return;
        }

        thread::sleep(Duration::from_secs(1));
        esp_restart();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn builds_excalibur_topics() {
        let project_id = "018f4c5c-9b4d-7cc2-a62a-44590f671001";
        let device_id = "018f4c5c-9b4d-7cc2-a62a-44590f671101";

        assert_eq!(
            telemetry_topic(project_id, device_id, "temperature"),
            format!("v1/p/{project_id}/d/{device_id}/telemetry/temperature")
        );
        assert_eq!(
            shadow_topic(project_id, device_id),
            format!("v1/p/{project_id}/d/{device_id}/shadow")
        );
        assert_eq!(
            commands_topic(project_id, device_id),
            format!("v1/p/{project_id}/d/{device_id}/commands")
        );
        assert_eq!(
            command_status_topic(project_id, device_id),
            format!("v1/p/{project_id}/d/{device_id}/commands/status")
        );
    }

    #[test]
    fn telemetry_payload_is_excalibur_json_array_without_legacy_id() {
        let payload = telemetry_payload(42, 1710760059006, json!({ "temperature": 24.5 })).unwrap();
        let value: Value = serde_json::from_slice(&payload).unwrap();

        assert_eq!(value[0]["sequence"], 42);
        assert_eq!(value[0]["timestamp"], 1710760059006u64);
        assert_eq!(value[0]["temperature"], 24.5);
        assert!(value[0].get("id").is_none());
    }

    #[test]
    fn shadow_payload_requires_an_object() {
        let payload = shadow_payload(json!({ "health": "nominal" })).unwrap();
        let value: Value = serde_json::from_slice(&payload).unwrap();

        assert_eq!(value, json!({ "health": "nominal" }));
        assert!(shadow_payload("bad").is_err());
    }

    #[test]
    fn command_status_payload_uses_action_id_and_clamps_progress() {
        let payload = command_status_payload(
            "018f4c5c-9b4d-7cc2-a62a-44590f671301",
            CommandState::Running,
            150,
            &[] as &[&str],
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&payload).unwrap();

        assert_eq!(
            value[0]["action_id"],
            "018f4c5c-9b4d-7cc2-a62a-44590f671301"
        );
        assert_eq!(value[0]["state"], "Running");
        assert_eq!(value[0]["progress"], 100);
        assert_eq!(value[0]["errors"], json!([]));
        assert!(value[0].get("id").is_none());
    }

    #[test]
    fn command_payload_keeps_json_value_shape() {
        let command: Command = serde_json::from_value(json!({
            "action_id": "018f4c5c-9b4d-7cc2-a62a-44590f671301",
            "name": "ota.install",
            "payload": {
                "component": "motor"
            }
        }))
        .unwrap();

        assert_eq!(command.action_id, "018f4c5c-9b4d-7cc2-a62a-44590f671301");
        assert_eq!(command.name, "ota.install");
        assert!(command.payload.is_object());
        assert!(!command.payload.is_string());
    }

    #[test]
    fn config_supports_inline_private_key_and_private_key_path() {
        let inline: DeviceConfig = serde_json::from_value(json!({
            "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
            "device_id": "018f4c5c-9b4d-7cc2-a62a-44590f671101",
            "broker": "mqtt.local",
            "port": 8883,
            "authentication": {
                "ca_certificate": "ca",
                "device_certificate": "cert",
                "device_private_key": "key"
            }
        }))
        .unwrap();
        assert_eq!(inline.authentication.private_key_pem().unwrap(), "key");

        let temp_dir = tempfile::tempdir().unwrap();
        let key_path = temp_dir.path().join("device.key");
        fs::write(&key_path, "path-key").unwrap();
        let key_path = key_path.display().to_string();
        let path_config: DeviceConfig = serde_json::from_value(json!({
            "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
            "device_id": "018f4c5c-9b4d-7cc2-a62a-44590f671101",
            "broker": "mqtt.local",
            "port": 8883,
            "authentication": {
                "ca_certificate": "ca",
                "device_certificate": "cert",
                "device_private_key_path": key_path
            }
        }))
        .unwrap();
        assert_eq!(
            path_config.authentication.private_key_pem().unwrap(),
            "path-key"
        );
    }

    #[test]
    fn ota_install_payload_validates_required_wire_fields() {
        let valid = OtaInstallPayload {
            firmware_id: "018f4c5c-9b4d-7cc2-a62a-44590f671201".to_owned(),
            component: "motor".to_owned(),
            version: "3.2.1".to_owned(),
            signed_url: "https://objects.example/firmware.bin?sig=1".to_owned(),
            sha256: "a".repeat(64),
            signature: None,
            size_bytes: 1024,
        };

        assert!(valid.validate().is_ok());
        assert!(OtaInstallPayload {
            sha256: "bad".to_owned(),
            ..valid.clone()
        }
        .validate()
        .is_err());
        assert!(OtaInstallPayload {
            size_bytes: 0,
            ..valid.clone()
        }
        .validate()
        .is_err());
        assert!(OtaInstallPayload {
            signed_url: "/firmware.bin".to_owned(),
            ..valid
        }
        .validate()
        .is_err());
    }
}
