# Excalibur

Excalibur is a Bytebeam-like IoT SaaS platform scaffold built around:

- Rust control plane with Axum
- Toasty-ready control-model boundary
- Broker-agnostic MQTT ingest/ACL library with a rumqttd adapter boundary
- Excalibur `device-agent`, productized from `bytebeamio/uplink` but using only Excalibur native protocol
- TimescaleDB for control-plane PostgreSQL tables and telemetry hypertables
- Bun, TypeScript, Next.js, and Tailwind for the operator console

The repository is intentionally split into backend, frontend, infrastructure, and simulator areas so the commercial platform can grow without tying telemetry ingestion to ORM abstractions.

This is a runnable commercial-platform scaffold, not a production-ready hosted service yet. The API currently uses an explicit `STORAGE_BACKEND=memory` development store; setting `STORAGE_BACKEND=postgres` or `timescale` fails fast until SQL repositories and shared sessions are implemented. MQTT topic authorization, telemetry decoding, shadow updates, command status parsing, device-agent auth JSON, CSR/dev provisioning, certificate revoke, and action payload validation are implemented as tested library/API code; the standalone `mqtt-ingest` binary is the process boundary where the production rumqttd hook, certificate auth, and NATS buffering will attach.

Excalibur is Bytebeam-like, not Bytebeam-compatible. Device topics, action wire shape, provisioning JSON, and control-plane APIs use Excalibur's native `v1/p/{project_id}/d/{device_id}/...` model. There is no `/tenants/...` topic compatibility layer.

## Repository Layout

```text
backend/                  Rust workspace
  apps/api                REST API and SSE control plane
  apps/mqtt-ingest        MQTT topic validation and ingest adapter boundary
  apps/worker             background jobs for alerts/actions/retention
  crates/domain           shared IoT domain model
  crates/device-protocol  MQTT topic and payload contracts
  crates/storage          storage traits plus in-memory development store
  migrations              TimescaleDB schema
device-agent/             Linux-first Excalibur device agent workspace
frontend/                 Next.js operator console
infra/helm/excalibur      Kubernetes Helm chart
examples/device-simulator Rust device simulator skeleton
docs/                     mdBook platform documentation
docker-compose.yml        local TimescaleDB/NATS/MinIO/app stack
```

## Documentation

The root documentation is organized with mdBook under `docs/`.

```bash
mdbook build docs
mdbook serve docs --open
```

Start with `docs/src/index.md` for the current scaffold status, architecture, API, device protocol, device-agent, infrastructure, security, operations, testing, and production roadmap.

## Local Development

Backend:

```bash
cd backend
cargo test --workspace
STORAGE_BACKEND=memory cargo run -p excalibur-api
```

Frontend:

```bash
cd frontend
bun install
bun run dev
```

Device agent:

```bash
cd device-agent
RUSTUP_TOOLCHAIN=stable cargo test --workspace
```

The device agent accepts auth JSON with `broker`, `port`, `project_id`, `device_id`, and nested `authentication` containing CA certificate, device certificate, and either inline `device_private_key` or `device_private_key_path`. Production provisioning should use `/api/v1/devices/{device_id}/provision/csr`; `/api/v1/devices/{device_id}/provision/dev-auth` is for local development and batch experiments only.

Infrastructure:

```bash
docker compose up timescaledb nats minio
```

The API starts with an in-memory store for fast local development. Apply `backend/migrations/001_initial.sql` to TimescaleDB before wiring the production SQL repositories; the Helm chart also includes a pre-install/pre-upgrade migration Job scaffold that runs the same initial SQL from `infra/helm/excalibur/migrations/001_initial.sql`.
