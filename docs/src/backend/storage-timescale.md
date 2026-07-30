# 存储与 TimescaleDB

Excalibur 使用一个 TimescaleDB 集群承载控制面 PostgreSQL 表和遥测 hypertable。控制面数据是关系模型；遥测数据是追加写入、高基数、按时间查询的时序模型。

## Migration

当前 schema 位于：

- `backend/migrations/001_initial.sql`
- `backend/migrations/002_sql_repository_upgrade.sql`
- `backend/migrations/003_auth_control_plane.sql`
- `backend/migrations/004_firmware_metadata.sql`
- `backend/migrations/005_m5_m8_operability.sql`
- `infra/helm/excalibur/migrations/001_initial.sql`
- `infra/helm/excalibur/migrations/002_sql_repository_upgrade.sql`
- `infra/helm/excalibur/migrations/003_auth_control_plane.sql`
- `infra/helm/excalibur/migrations/004_firmware_metadata.sql`
- `infra/helm/excalibur/migrations/005_m5_m8_operability.sql`

它包含：

- TimescaleDB extension。
- `pgcrypto` extension。
- enum 类型：`member_role`、`device_status`、`certificate_status`、`action_state`、`alert_kind`、`alert_event_state`、`diagnostics_session_state`、`firmware_rollout_state`。
- 控制面表：`users`、`orgs`、`memberships`、`projects`、`devices`、`device_certificates`、`stream_definitions`、`actions`、`action_targets`、`firmware_artifacts`、`firmware_rollouts`、`dashboards`、`alert_rules`、`alert_events`、`diagnostics_sessions`、`audit_logs`。
- Auth 控制面表：`user_sessions`、`used_refresh_tokens`、`api_keys`。
- 遥测表：`telemetry_points` hypertable。
- 遥测去重表：`telemetry_sequence_dedup`，按 `(project_id, device_id, stream, sequence)` 保证重放幂等。
- Helm migration runner 元数据表：`schema_migrations`、`schema_migration_events`。

## 控制面表

控制面表按租户层级建模：

```text
orgs
  memberships
  projects
    devices
      device_certificates
    stream_definitions
    telemetry_points
    actions
      action_targets
    firmware_artifacts
      firmware_rollouts
    dashboards
    alert_rules
      alert_events
    diagnostics_sessions
    api_keys
  audit_logs
  api_keys
users
  user_sessions
    used_refresh_tokens
```

关键约束：

- `users` 保留 `UNIQUE (email)`，并额外用 `users_email_lower_unique_idx` 强制大小写不敏感的邮箱唯一性。
- `projects` 使用 `UNIQUE (org_id, slug)`。
- `projects` 额外使用 `UNIQUE (org_id, id)`，为 audit log 的 org/project 复合外键提供租户约束。
- `devices` 使用 `UNIQUE (project_id, id)`，为复合外键提供租户约束。
- `device_certificates` 通过 `(project_id, device_id)` 引用 devices，避免跨项目证书绑定。
- `user_sessions` 保存 access token hash、refresh token hash、过期时间、撤销时间和 last-used 时间；`used_refresh_tokens` 保存已轮换 refresh token hash，用于复用检测。
- `api_keys` 保存 key hash、org/project scope、scopes、过期时间、撤销时间和 last-used 时间；明文 key 只在创建响应返回一次。
- `actions` 使用 `UNIQUE (project_id, id)`，`action_targets` 通过 `(project_id, action_id)` 和 `(project_id, device_id)` 绑定作用域。
- `audit_logs` 通过 `(org_id, project_id)` 引用 projects，避免 audit entry 绑定到错误 org。
- `audit_logs_scope_idx` 和 `audit_logs_org_created_idx` 支持 project-scoped 和 org-scoped 最新日志查询。

## Helm migration runner

Helm chart 的 pre-install/pre-upgrade migration Job 会：

- 初始化 `schema_migrations` 和 `schema_migration_events`。
- 对每个 migration 使用 `pg_advisory_lock(hashtext('excalibur_schema_migrations'))` 串行化执行，避免多个 Helm release/upgrade 同时写 schema。
- 执行前写入 `applying` 事件，成功后在同一事务内写入 `schema_migrations` 并更新为 `applied`，失败时写入 `failed` 和恢复提示。
- 保留旧集群兼容：如果已存在完整的 001 核心表但没有 migration 记录，会自动把 `001_initial.sql` 标记为已应用；如果只存在部分旧表，Job 会失败并提示缺失对象，避免把半初始化 schema 误标为已迁移。
- `002_sql_repository_upgrade.sql` 会在加 case-insensitive email unique index 和 audit org/project 外键前检查冲突数据，失败时输出需要清理的样例。
- `002_sql_repository_upgrade.sql` 仍包含 telemetry index 调整和 dedupe 回填；已有大量 telemetry 的生产库应在维护窗口执行，或先拆成后续 online DDL/分块 backfill 方案。
- 通过 `migrations.activeDeadlineSeconds` 限制 Job 最长运行时间，避免锁等待无限挂住 release。

失败恢复：

1. 查看 Job 日志和 `schema_migration_events` 最新 `failed` 记录。
2. 修复数据库状态或 migration SQL。
3. 重新执行 Helm upgrade；已写入 `schema_migrations` 的版本会跳过，失败版本会重新尝试。

## Telemetry hypertable

`telemetry_points` 字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `project_id` | UUID | 租户查询和隔离主键之一。 |
| `device_id` | UUID | 设备身份。 |
| `stream` | TEXT | 动态 stream 名称。 |
| `sequence` | BIGINT | 设备端递增序号，用于去重和排序辅助。 |
| `ts` | TIMESTAMPTZ | 设备端时间戳，hypertable 时间维度。 |
| `payload` | JSONB | stream payload fields。 |
| `ingested_at` | TIMESTAMPTZ | 平台接收时间。 |

Hypertable primary key：

```sql
PRIMARY KEY (project_id, device_id, stream, sequence, ts)
```

TimescaleDB 要求 hypertable 的唯一约束包含时间维度，因此逻辑去重不直接依赖该 primary key。写入路径会先插入 `telemetry_sequence_dedup`：

```sql
PRIMARY KEY (project_id, device_id, stream, sequence)
```

只有新 sequence key 才会进入 `telemetry_points`；同一个设备、stream、sequence 即使带不同 timestamp 重放，也会被忽略。

Indexes：

```sql
CREATE INDEX telemetry_points_project_ts_idx
  ON telemetry_points (project_id, ts DESC, sequence DESC);

CREATE INDEX telemetry_points_project_stream_ts_idx
  ON telemetry_points (project_id, stream, ts DESC, sequence DESC);

CREATE INDEX telemetry_points_project_device_ts_idx
  ON telemetry_points (project_id, device_id, ts DESC, sequence DESC);

CREATE INDEX telemetry_points_project_device_stream_ts_idx
  ON telemetry_points (project_id, device_id, stream, ts DESC, sequence DESC);
```

Timescale policies：

```sql
ALTER TABLE telemetry_points SET (
  timescaledb.compress,
  timescaledb.compress_segmentby = 'project_id,device_id,stream',
  timescaledb.compress_orderby = 'ts DESC'
);

SELECT add_compression_policy('telemetry_points', INTERVAL '7 days', if_not_exists => TRUE);
SELECT add_retention_policy('telemetry_points', INTERVAL '180 days', if_not_exists => TRUE);
```

## Toasty 使用边界

Toasty 只适合控制面强模型。遥测路径必须绕开 ORM，原因是：

- MQTT ingest 需要批量写入和 backpressure。
- Timescale hypertable、compression、retention、continuous aggregate 需要 raw SQL 或 migration 管理。
- Dashboard 查询常用窗口聚合和 downsampling，不适合被通用 ORM 隐藏。

当前实现方式：

- 控制面 repositories 已在 `backend/crates/storage` 中通过 SQLx raw queries 实现。
- Telemetry ingest 使用 SQLx chunked bulk insert，并通过 `telemetry_sequence_dedup` 做 sequence 幂等；后续更高吞吐路径可替换为 COPY 或专用 writer。
- Timescale policies、continuous aggregates 和 retention 通过 migration 管理。

## SQL repository 实现

`backend/crates/storage` 提供两个后端：

- `MemoryStore`：开发和单元测试使用。
- `PgStore`：SQL repository，连接 PostgreSQL/TimescaleDB。

API 通过统一 `Store` enum 调用 repository 方法，`STORAGE_BACKEND=timescale` 会创建 `PgStore` 并校验 TimescaleDB schema。SQL repository 覆盖：

- users、orgs、memberships、projects。
- devices、device_certificates、shadow/online heartbeat。
- user_sessions、used_refresh_tokens、api_keys。
- stream definitions。
- telemetry_points 写入和查询。
- actions 与 action_targets；父 action 状态和进度从所有 target 聚合，避免单个设备完成时把批量 action 误标为完成。
- firmware_artifacts 与 firmware_rollouts。
- dashboards。
- alert_rules 与 alert_events。
- diagnostics_sessions。
- audit_logs。

SQL-backed 启动：

```bash
DATABASE_URL=postgres://excalibur:excalibur@localhost:5432/excalibur \
  STORAGE_BACKEND=timescale \
  cargo run -p excalibur-api
```

本地 SQL contract test：

```bash
EXCALIBUR_SQL_TEST_DATABASE_URL=postgres://excalibur:excalibur@localhost:5432/excalibur \
  RUSTUP_TOOLCHAIN=stable \
  cargo test -p excalibur-storage pg_store_contract_runs_when_database_url_is_set -- --nocapture
```

未设置 `EXCALIBUR_SQL_TEST_DATABASE_URL` 时，本地测试会跳过 live SQL contract。设置该变量后，storage contract 会覆盖 SQL schema validation、tenant scope、session rotation/reuse detection、API key scope/revoke、active certificate fingerprint lookup、telemetry sequence 去重、多 target action 聚合、firmware finalize/rollout、alert event 和 diagnostics session；mqtt-ingest 也有同变量门控的 SQL-backed ingest contract。CI workflow 会启动 TimescaleDB 并设置该变量，因此 SQL contract 在 CI 中强制执行。

## 生产 repository 要求

SQL repository 必须满足：

- 每个 project-scoped 查询都显式带 `project_id`。
- 写入前检查外键和 project scope。
- 对创建 action 和 action_targets 使用事务。
- 对 telemetry ingest 使用事务，并通过 `telemetry_sequence_dedup` 对重复 `(project_id, device_id, stream, sequence)` 执行 `ON CONFLICT DO NOTHING`。
- 启动 schema validation 必须验证 `telemetry_points` 是 Timescale hypertable，并且 compression/retention policy 已存在。
- 对 audit log 使用 append-only 语义。
- 对 API key、refresh token、device certificate fingerprint 只保存 hash 或 fingerprint，不保存敏感明文。

## 查询形态

Dashboard/query API 当前支持 telemetry bucket aggregate；后续应继续支持：

- raw rows：按 device、stream、time range 查询。
- latest：按 device/stream 查最新值。
- export：CSV/Parquet 文件写入对象存储。
- pagination：基于 `(ts, sequence)` cursor。

任何查询默认都必须带 project scope。
