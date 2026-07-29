# Excalibur ESP Rust Provisioning

This helper writes `device_config.json` into the ESP SPIFFS partition used by `ExcaliburClient::init()`.

1. Copy `device_config.json.example` to `device_config.json`.
2. Replace the placeholder project ID, device ID, broker, and certificates with values from Excalibur provisioning.
3. Flash the helper:

```sh
cargo espflash --release --monitor --partition-table partitions.csv
```

Use the same partition table for the provisioning helper and the application that reads `/spiffs/device_config.json`.
