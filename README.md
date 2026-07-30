# Excalibur

Excalibur is a Bytebeam-like IoT SaaS platform scaffold built around:

- Rust control plane with Axum
- Toasty-ready control-model boundary
- `rumqttd` MQTT broker runtime with Excalibur topic ingest
- Excalibur `device-agent`, productized from `bytebeamio/uplink` but using only Excalibur native protocol
- TimescaleDB for control-plane PostgreSQL tables and telemetry hypertables
- Bun, TypeScript, Next.js, and Tailwind for the operator console

The repository is intentionally split into backend, frontend, infrastructure, and simulator areas so the commercial platform can grow without tying telemetry ingestion to ORM abstractions.

This is a runnable commercial-platform scaffold, not a production-ready hosted service yet. The API can run with `STORAGE_BACKEND=memory` for fast development or `STORAGE_BACKEND=timescale` backed by SQL repositories. SQL mode stores hashed access sessions, rotating refresh tokens, used refresh-token replay markers, and hashed API keys durably; memory mode remains process-local for development. MQTT topic authorization, telemetry decoding, shadow updates, command status parsing, device-agent auth JSON, CSR/dev provisioning, certificate revoke, action payload validation, telemetry aggregate queries, alert events, diagnostics sessions, firmware finalize/rollout, and API metrics/rate-limit baseline are implemented as tested library/API code. The standalone `mqtt-ingest` binary starts a local MQTT v4 listener with `rumqttd`, supports TLS peer-certificate fingerprint identity, consumes Excalibur native publish topics into Store or JetStream, and bridges worker commands back to device topics.

Excalibur is Bytebeam-like, not Bytebeam-compatible. Device topics, action wire shape, provisioning JSON, and control-plane APIs use Excalibur's native `v1/p/{project_id}/d/{device_id}/...` model. There is no `/tenants/...` topic compatibility layer.

## Repository Layout

```text
backend/                  Rust workspace
  apps/api                REST API and SSE control plane
  apps/mqtt-ingest        rumqttd MQTT broker and topic ingest runtime
  apps/worker             background jobs for alerts/actions/retention
  crates/domain           shared IoT domain model
  crates/device-protocol  MQTT topic and payload contracts
  crates/storage          MemoryStore and SQLx PgStore repositories
  migrations              TimescaleDB schema
device-agent/             Linux-first Excalibur device agent workspace
frontend/                 Next.js operator console
infra/helm/excalibur      Kubernetes Helm chart
examples/device-simulator Rust device simulator skeleton
docs/                     mdBook platform documentation
docker-compose.yml        local TimescaleDB/NATS/RustFS/app stack
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

SQL-backed API:

```bash
docker compose up -d timescaledb nats rustfs
cd backend
DATABASE_URL=postgres://excalibur:excalibur@localhost:5432/excalibur \
  STORAGE_BACKEND=timescale \
  cargo run -p excalibur-api
```

MQTT broker and ingest runtime:

```bash
make mqtt
```

End-to-end local app processes:

```bash
make dev-full
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
docker compose up timescaledb nats rustfs
```

The API starts with an in-memory store for fast local development unless `STORAGE_BACKEND=timescale` is set. SQL-backed startup requires `DATABASE_URL` and an initialized TimescaleDB schema from `backend/migrations/*.sql`; Docker Compose initializes new TimescaleDB volumes automatically, and the Helm chart includes a pre-install/pre-upgrade migration Job runner that applies versioned SQL files from `infra/helm/excalibur/migrations/`.
