# Excalibur ESP-IDF Examples

The SDK keeps a minimal example set:

- `telemetry`: publishes one telemetry record and one shadow snapshot.
- `commands`: subscribes to Excalibur commands and handles a `toggle` command.
- `ota`: registers the built-in `ota.install` handler.

Each example references the SDK through `main/idf_component.yml` with `override_path`.
