# Excalibur ESP-IDF SDK

ESP-IDF component for the Excalibur native device protocol.

## Use From This Repository

An ESP-IDF app can reference this component with `EXTRA_COMPONENT_DIRS`:

```cmake
set(EXTRA_COMPONENT_DIRS "/path/to/excalibur/sdk/excalibur-esp-idf-sdk")
```

or with an ESP Component Manager manifest:

```yaml
dependencies:
  excalibur-esp-idf-sdk:
    override_path: "../../../sdk/excalibur-esp-idf-sdk"
```

Include the public SDK header:

```c
#include "excalibur_sdk.h"
```

## Protocol

The component publishes Excalibur native MQTT topics:

- telemetry: `v1/p/{project_id}/d/{device_id}/telemetry/{stream}`
- shadow: `v1/p/{project_id}/d/{device_id}/shadow`
- commands: `v1/p/{project_id}/d/{device_id}/commands`
- command status: `v1/p/{project_id}/d/{device_id}/commands/status`

Telemetry helpers accept a JSON object, add `sequence` and `timestamp`, and publish a JSON array. Shadow helpers publish a single JSON object. Command status publishes a JSON array containing `action_id`, `state`, `progress`, and `errors`.

## Device Config

`excalibur_init()` reads `/spiffs/device_config.json` unless `use_device_config_data` is set and `device_cfg` is filled by the application.

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

`authentication.device_private_key_path` is also supported.

## Examples

The `examples/` directory contains the minimal supported set: telemetry, commands, and OTA.
