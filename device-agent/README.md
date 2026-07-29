# Excalibur Device Agent

`device-agent` is Excalibur's Linux-first device runtime, productized from `bytebeamio/uplink` and rewritten for Excalibur's native MQTT protocol.

It is not Bytebeam protocol compatible. The agent publishes and subscribes only on Excalibur topics:

- telemetry: `v1/p/{project_id}/d/{device_id}/telemetry/{stream}`
- shadow: `v1/p/{project_id}/d/{device_id}/shadow`
- commands: `v1/p/{project_id}/d/{device_id}/commands`
- command status: `v1/p/{project_id}/d/{device_id}/commands/status`

## Capabilities

- Reliable telemetry buffering with disk persistence.
- Device shadow snapshots.
- JSON command payloads where `action_id` is a UUID string and `payload` is a JSON value.
- OTA/download handoff for `ota.install`.
- Diagnostics/log/system-stat streams.
- Remote shell code retained as a gated beta path; it is disabled by default.
- TLS auth JSON with inline private key for development or `device_private_key_path` for CSR provisioning.

## Auth JSON

Production devices should generate their keypair locally, submit a CSR to Excalibur, and keep the private key on disk:

```json
{
  "broker": "mqtt.local.excalibur.dev",
  "port": 8883,
  "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
  "device_id": "018f4c5c-9b4d-7cc2-a62a-44590f671101",
  "authentication": {
    "ca_certificate": "-----BEGIN CERTIFICATE-----\\n...\\n-----END CERTIFICATE-----",
    "device_certificate": "-----BEGIN CERTIFICATE-----\\n...\\n-----END CERTIFICATE-----",
    "device_private_key_path": "/etc/excalibur/device.key"
  }
}
```

Development and batch experiments may use `/api/v1/devices/{device_id}/provision/dev-auth`, which returns an inline `device_private_key`. Do not use that path for production fleets.

## Development

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo test --workspace
```

The repository keeps `device-agent` as an independent Rust workspace so its upstream-derived dependencies and Android remnants do not affect backend builds. Android code is retained but Linux checks are the first-stage acceptance target.
