# 存储与 TimescaleDB

Excalibur 使用一个 TimescaleDB 集群承载控制面 PostgreSQL 表和遥测 hypertable。控制面数据是关系模型；遥测数据是追加写入、高基数、按时间查询的时序模型。

## Migration

当前 schema 位于：

- `backend/migrations/001_initial.sql`
- `infra/helm/excalibur/migrations/001_initial.sql`

它包含：

- TimescaleDB extension。
- `pgcrypto` extension。
- enum 类型：`member_role`、`device_status`、`certificate_status`、`action_state`、`alert_kind`。
- 控制面表：`users`、`orgs`、`memberships`、`projects`、`devices`、`device_certificates`、`stream_definitions`、`actions`、`action_targets`、`firmware_artifacts`、`dashboards`、`alert_rules`、`audit_logs`。
- 遥测表：`telemetry_points` hypertable。

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
    dashboards
    alert_rules
  audit_logs
```

关键约束：

- `projects` 使用 `UNIQUE (org_id, slug)`。
- `devices` 使用 `UNIQUE (project_id, id)`，为复合外键提供租户约束。
- `device_certificates` 通过 `(project_id, device_id)` 引用 devices，避免跨项目证书绑定。
- `actions` 使用 `UNIQUE (project_id, id)`，`action_targets` 通过 `(project_id, action_id)` 和 `(project_id, device_id)` 绑定作用域。
- `audit_logs_scope_idx` 支持 org/project 范围查询。

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

Primary key：

```sql
PRIMARY KEY (project_id, device_id, stream, sequence, ts)
```

Indexes：

```sql
CREATE INDEX telemetry_points_project_stream_ts_idx
  ON telemetry_points (project_id, stream, ts DESC);

CREATE INDEX telemetry_points_device_ts_idx
  ON telemetry_points (device_id, ts DESC);
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

推荐实现方式：

- 控制面 repositories 可在 `backend/crates/storage` 中引入 Toasty 或 SQLx。
- Telemetry ingest 使用 SQLx raw query、COPY、prepared batch insert 或专用 writer。
- Timescale policies、continuous aggregates 和 retention 通过 migration 管理。

## 生产 repository 要求

SQL repository 必须满足：

- 每个 project-scoped 查询都显式带 `project_id`。
- 写入前检查外键和 project scope。
- 对创建 action 和 action_targets 使用事务。
- 对 certificate revoke 使用事务并保持幂等。
- 对 telemetry ingest 支持批量写入和重复 sequence 冲突策略。
- 对 audit log 使用 append-only 语义。
- 对 API key、refresh token、device certificate fingerprint 只保存 hash 或 fingerprint，不保存敏感明文。

## 查询形态

Dashboard/query API 后续应支持：

- raw rows：按 device、stream、time range 查询。
- aggregate rows：按 interval downsample，例如 1m、5m、1h。
- latest：按 device/stream 查最新值。
- export：CSV/Parquet 文件写入对象存储。
- pagination：基于 `(ts, sequence)` cursor。

任何查询默认都必须带 project scope。
