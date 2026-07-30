---
date: 2026-07-30
topic: roadmap-next-requirements
focus: next production roadmap requirements after M2-M8 baseline
---

# Ideation: Roadmap Next Requirements

## Codebase Context

Excalibur is now a runnable IoT SaaS scaffold with Rust/Axum API, SQLx/Timescale storage, rumqttd MQTT runtime, NATS/JetStream buffering, worker jobs, RustFS/S3-style object flows, device-agent support, Helm/Compose infrastructure, and a Next.js console. The recently closed roadmap work implemented persistent sessions/API keys, real device certificates, MQTT identity ACL, telemetry aggregate query, JetStream ingest worker, action dispatch, firmware rollout metadata, diagnostics sessions, alert events, basic metrics, and production docs.

The remaining highest-leverage gaps are no longer broad scaffolding gaps. They are reliability and production hardening gaps: durable MQTT ack semantics, durable command dispatch, real alert delivery, object lifecycle verification, export/continuous aggregate paths, secret/rate-limit hardening, mTLS/load CI, and Console workflow completion.

## Ranked Ideas

### 1. Durable MQTT Ingest ACK/Outbox

**Description:** Bind MQTT QoS1 PUBACK to durable ingest acknowledgement by introducing an ingest outbox or broker-to-JetStream ack bridge. The broker should only acknowledge device telemetry once the payload is durably accepted by JetStream or an equivalent local outbox.

**Rationale:** The roadmap explicitly calls out the current loss window where broker ACK can happen before JetStream/storage durability. Closing this makes telemetry delivery semantics defensible before scale testing.

**Downsides:** Requires careful rumqttd adapter changes, backpressure handling, duplicate/retry semantics, and simulator coverage for broker restart and NATS outage.

**Confidence:** 92%

**Complexity:** High

**Status:** Unexplored

### 2. Durable Action/OTA Command Bus With JIT Object URLs

**Description:** Move action command dispatch from plain NATS publish to a JetStream durable subject/consumer, ack only after MQTT bridge publish succeeds, and persist firmware/diagnostics references instead of short TTL signed URLs. The worker should sign object URLs immediately before dispatch.

**Rationale:** This closes the main command-loss and URL-expiry risks in Actions/OTA. It also gives rollout, retry, and bridge restart behavior a single durable state model.

**Downsides:** Needs command stream design, bridge restart tests, payload migration, and careful redaction/audit boundaries.

**Confidence:** 90%

**Complexity:** High

**Status:** Unexplored

### 3. Real Alert Notification Providers

**Description:** Add a notification worker/provider layer that consumes alert notification subjects and sends webhook/email notifications with retry, backoff, provider result metrics, and durable attempt records.

**Rationale:** Alert events already open/resolve and track notification attempts, but the notification subject is not connected to real providers. This is a natural next product loop.

**Downsides:** Provider configuration, secrets, template safety, and retry idempotency need discipline. Email deliverability is a separate operational concern.

**Confidence:** 84%

**Complexity:** Medium

**Status:** Unexplored

### 4. Diagnostics Object Verification And Lifecycle

**Description:** On diagnostics finalize, verify object size/hash against object storage instead of trusting the caller, then add retention/lifecycle policy and a sweeper for expired diagnostics artifacts.

**Rationale:** Diagnostics sessions already support signed upload/download and audit. Verifying the actual uploaded object and expiring old files closes a security and cost-control gap.

**Downsides:** Requires object HEAD/checksum behavior across S3-compatible stores and clear failure states when metadata does not match.

**Confidence:** 86%

**Complexity:** Medium

**Status:** Unexplored

### 5. Telemetry Analytics And Export Layer

**Description:** Add production query/export primitives: continuous aggregate or cache-backed dashboard queries, CSV export first, Parquet export next, and object-storage-backed export job metadata.

**Rationale:** Dashboard aggregate exists, but roadmap still lists export and high-throughput aggregate productionization. Export is also useful for customer support and enterprise evaluation.

**Downsides:** Parquet adds dependency and schema decisions. Continuous aggregates need migration strategy and retention alignment.

**Confidence:** 78%

**Complexity:** Medium

**Status:** Unexplored

### 6. Operations And Security Hardening Pack

**Description:** Add ExternalSecret/KMS-ready Helm values for CA/S3/API secrets, trusted-proxy configuration for client IP rate limits, API key quotas, MQTT connect/publish rate limits, and broader Prometheus metrics/log correlation.

**Rationale:** The docs still call out secret management, abuse controls, and missing metrics as commercial SaaS requirements. This is a concrete path from scaffold to staging-readiness.

**Downsides:** Some work depends on deployment environment conventions. Rate limits must avoid breaking simulators and local development.

**Confidence:** 82%

**Complexity:** Medium

**Status:** Unexplored

### 7. mTLS Simulator CI And Load/Resilience Harness

**Description:** Add simulator-driven CI for CSR/dev-auth/mTLS connect, revoked/disabled/cross-project denial, telemetry publish, command status, and NATS/MQTT/DB failure scenarios. Extend the existing load smoke script toward MQTT mTLS load testing.

**Rationale:** The implementation is broad, but much of the current confidence is unit/contract based. A CI matrix catches the integration regressions most likely to break production onboarding.

**Downsides:** Requires reliable local service orchestration and careful runtime budgets so CI does not become flaky.

**Confidence:** 88%

**Complexity:** Medium

**Status:** Unexplored

## Rejection Summary

| # | Idea | Reason Rejected |
|---|------|-----------------|
| 1 | Remote Shell Beta immediately | High-risk capability should wait until durable command bus, RBAC/audit hardening, and abuse controls are stronger. |
| 2 | Full marketing/landing page | Not aligned with the current production SaaS backend runway. |
| 3 | Native mobile app | Not grounded in the repo's current operator-console and device-agent priorities. |
| 4 | Replace nats-lite with official async-nats as a standalone requirement | Valuable, but better handled inside durable ingest/command bus work rather than as an isolated dependency swap. |
| 5 | Timescale COPY-only optimization | Too narrow alone; should sit under telemetry analytics/ingest productionization after ACK semantics are fixed. |
| 6 | Console visual redesign | The real gap is workflow/page/RBAC/E2E completion, not appearance-first redesign. |
| 7 | Multi-cloud object storage abstraction | Current S3-compatible boundary is enough; verification/lifecycle gives more immediate value. |
| 8 | Stream schema registry as a large standalone platform | Useful later, but export/aggregate and alert validation can start with smaller stream-definition extensions. |

## Session Log

- 2026-07-30: Initial ideation - 18 candidates generated, 7 survived after production-roadmap filtering.
