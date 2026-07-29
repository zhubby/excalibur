# 生产化路线图

当前仓库是可运行商业平台 scaffold。要达到可上线商业 SaaS，需要按风险和依赖顺序推进。

## Milestone 1: 持久控制面

目标：

- 实现 SQL repositories。
- API 从 `MemoryStore` 切换到 TimescaleDB/PostgreSQL。
- 保持所有 tenant scope 测试。
- 引入 migration runner。

验收：

- `STORAGE_BACKEND=timescale` 可启动。
- backend tests 同时覆盖 memory 和 SQL。
- org/project/device/action/audit 数据重启后保留。

## Milestone 2: 真实 PKI 与 MQTT runtime

目标：

- CA signing service。
- CSR 签发真实 cert。
- Certificate fingerprint 存储。
- rumqttd connect/publish/subscribe hook。
- Revocation 生效。

验收：

- mTLS device simulator 可连接。
- revoked cert connect 失败。
- cross project publish/subscribe 被拒绝。
- invalid payload 不写入。

## Milestone 3: Telemetry 写入链路

目标：

- NATS JetStream ingest buffer。
- Worker batch writer。
- Timescale COPY 或 batch insert。
- Duplicate sequence 策略。
- Continuous aggregates。

验收：

- telemetry p95 达标。
- NATS lag 可观测。
- retention/compression policy 生效。
- Dashboard query 不受 raw 写入明显拖慢。

## Milestone 4: Actions 与 OTA

目标：

- Action target 持久状态。
- Dispatcher 发布 commands。
- Firmware upload 和 S3 signed URL。
- OTA approval、retry、timeout、cancel。
- Agent installer contract。

验收：

- 批量 OTA 状态可追踪。
- checksum/signature 错误失败。
- timeout/retry 行为稳定。
- audit 覆盖所有危险操作。

## Milestone 5: Dashboard、Alerts、Diagnostics

目标：

- Dashboard query API。
- CSV/Parquet export。
- Offline/threshold/window aggregation alerts。
- Email/webhook notification provider。
- Diagnostics session 和 object upload。

验收：

- dashboard time range 和 aggregate 正确。
- alert 去重和恢复。
- notification retry 可观测。
- diagnostics 文件可下载且审计完整。

## Milestone 6: Console 生产化

目标：

- OpenAPI generated TS client。
- Auth/session integration。
- Project switch。
- 真实 device list/detail。
- Streams/dashboard/action/firmware/security 页面。
- RBAC route/button enforcement。

验收：

- 前端 E2E 覆盖关键工作流。
- 无静态 demo 数据依赖。
- 错误态、加载态、空态完整。

## Milestone 7: Remote Shell Beta

目标：

- Excalibur WebSocket tunnel。
- Short-lived authorization。
- Project beta flag。
- RBAC + audit。
- Session kill。

验收：

- 未启用 beta 时 API 和 UI 都拒绝。
- session expires 后不可复用。
- 打开、关闭、命令 metadata audit 完整。
- 并发和来源限制生效。

## Milestone 8: 商业运维基线

目标：

- Metrics/logs/traces。
- Backup/restore。
- Rate limit。
- Secret management。
- Load test。
- Helm production values。

验收：

- 10 万设备目标压测报告。
- 故障演练报告。
- Runbook 完整。
- Go/No-Go checklist 可执行。
