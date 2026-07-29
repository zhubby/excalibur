# Excalibur ESP Rust SDK

Rust SDK for connecting ESP boards to the Excalibur native device protocol.

## Use From This Repository

Add the SDK as a path dependency from an ESP Rust application:

```toml
[dependencies]
excalibur-esp-rs = { path = "sdk/excalibur-esp-rs-sdk" }
```

The Rust import path is `excalibur_esp_rs`:

```rust
use excalibur_esp_rs::{Command, CommandState, ExcaliburClient};
```

## Device Config

`ExcaliburClient::init()` reads `/spiffs/device_config.json`.

```json
{
  "broker": "mqtt.local.excalibur.dev",
  "port": 8883,
  "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
  "device_id": "018f4c5c-9b4d-7cc2-a62a-44590f671101",
  "authentication": {
    "ca_certificate": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----",
    "device_certificate": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----",
    "device_private_key": "-----BEGIN PRIVATE KEY-----\n...\n-----END PRIVATE KEY-----"
  },
  "provisioning_mode": "Csr",
  "production": true
}
```

`authentication.device_private_key_path` is also supported when the key is stored as a separate local file.

## Protocol Helpers

The SDK publishes Excalibur native topics:

- telemetry: `v1/p/{project_id}/d/{device_id}/telemetry/{stream}`
- shadow: `v1/p/{project_id}/d/{device_id}/shadow`
- commands: `v1/p/{project_id}/d/{device_id}/commands`
- command status: `v1/p/{project_id}/d/{device_id}/commands/status`

Telemetry payloads are JSON arrays. Shadow payloads are single JSON objects. Command status payloads are JSON arrays with `action_id`, `state`, `progress`, and `errors`.

## Examples

Configure Wi-Fi in `cfg.toml`, provision `/spiffs/device_config.json`, then build an example:

```sh
cargo espflash --release --monitor --partition-table ./partitions_example.csv --example streams
cargo espflash --release --monitor --partition-table ./partitions_example.csv --example actions
cargo espflash --release --monitor --partition-table ./partitions_example.csv --example ota
```

Host protocol tests do not require ESP-IDF:

```sh
cargo test --manifest-path sdk/excalibur-esp-rs-sdk/Cargo.toml --no-default-features --lib
```
