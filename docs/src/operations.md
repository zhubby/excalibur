# 运维手册

本页描述生产运维目标和当前 scaffold 的运维接入点。

## 服务健康

| 服务 | 健康信号 |
| --- | --- |
| API | `GET /health` 返回 `{"status":"ok","service":"excalibur-api"}`。 |
| MQTT ingest | 进程启动日志包含 adapter ready；生产应暴露 metrics/health。 |
| Worker | 当前每 30s debug heartbeat；生产应暴露 queue lag、job count、error rate。 |
| Frontend | Next.js build 和 HTTP readiness。 |
| TimescaleDB | `pg_isready`、连接池、chunk/retention/compression 状态。 |
| NATS | JetStream health、consumer lag、ack pending。 |
| RustFS/S3 | bucket availability、signed URL success、object age。 |

## 日志

Rust 服务使用 `tracing` 和 env filter：

- API 默认：`excalibur_api=info,tower_http=info`
- MQTT ingest 默认：`excalibur_mqtt_ingest=info`
- Worker 默认：`excalibur_worker=info`

生产日志字段建议：

- `request_id`
- `org_id`
- `project_id`
- `actor_id`
- `device_id`
- `action_id`
- `certificate_id`
- `stream`
- `error.kind`

不要记录 private key、full certificate PEM、signed URL query、password、session token、API key 明文。

## Metrics

建议核心指标：

| 指标 | 维度 |
| --- | --- |
| `api_requests_total` | route、method、status。 |
| `api_request_duration_seconds` | route、method。 |
| `mqtt_connections_current` | project。 |
| `mqtt_connect_failures_total` | reason。 |
| `mqtt_publish_total` | project、stream、result。 |
| `ingest_batch_size` | stream。 |
| `ingest_lag_seconds` | stream。 |
| `timescale_write_duration_seconds` | table。 |
| `nats_consumer_lag` | stream、consumer。 |
| `action_state_total` | action name、state。 |
| `ota_failures_total` | component、reason。 |
| `alert_notifications_total` | provider、result。 |

## Alerts

平台自身应告警：

- API 5xx rate 升高。
- MQTT connect failures 激增。
- NATS consumer lag 超阈值。
- Timescale write p95 升高。
- Telemetry ingest lag 超阈值。
- Disk usage 接近 retention 压缩失效风险。
- Worker action timeout backlog 积压。
- S3 signed URL 或 upload failure。
- Certificate revoke 未能同步到 broker cache。

## Backup 与恢复

TimescaleDB：

- WAL archive。
- 每日 base backup。
- 定期恢复演练。
- 验证 hypertable chunk、compression policy 和 retention policy。

Object storage：

- Firmware bucket 开启 versioning 或 immutable retention。
- Diagnostics/export bucket 可以短保留，但要有生命周期策略。
- Signed URL 不作为长期引用，长期引用应是 object key。

NATS：

- JetStream store 要有磁盘容量监控。
- 关键 stream 配置副本数。
- 处理 dead-letter subject。

## Timescale 维护

定期检查：

```sql
SELECT * FROM timescaledb_information.hypertables;
SELECT * FROM timescaledb_information.jobs;
SELECT * FROM timescaledb_information.compression_settings;
```

关注：

- Compression job 是否成功。
- Retention job 是否过早删除 dashboard 需要的数据。
- Chunk 数量是否过多。
- Query 是否命中合适 index。

## 10 万设备目标的压测方向

阶段性压测应覆盖：

- 连接数：10 万 MQTT TLS 连接。
- 发布速率：按设备频率和 stream 分布建模。
- Batch insert：worker 到 Timescale p95 和错误率。
- Query：dashboard raw/aggregate p95。
- Action dispatch：批量 OTA command fanout。
- Failure：数据库慢写、NATS broker 重启、MQTT reconnect storm。

验收不能只看平均值，应看 p95/p99、重试堆积和恢复时间。
