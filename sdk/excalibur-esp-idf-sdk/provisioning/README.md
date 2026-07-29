# Excalibur ESP-IDF Provisioning

Create `device_config.json` from `spiffs_provisioning/config_data/device_config.json.example` and flash it into the same SPIFFS partition used by your application.

The SDK reads `/spiffs/device_config.json` by default.
