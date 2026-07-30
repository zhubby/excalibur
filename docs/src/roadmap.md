# 生产化路线图

当前仓库是可运行商业平台 scaffold。要达到可上线商业 SaaS，需要按风险和依赖顺序推进。

## Milestone 1: 持久控制面

状态：SQL repository 已实现；session/refresh token/API key 已迁到 SQL-backed store；Helm migration runner 已补 advisory lock、applied/failed audit 和失败恢复说明；API key scope enforcement 和 Console HttpOnly cookie session 已接入。

剩余目标：

- 保持所有 tenant scope 测试。
- 扩展 API key scope 到后续 worker/MQTT ingest 服务间调用。

验收：

- `STORAGE_BACKEND=timescale` 可启动。
- backend tests 覆盖 memory；CI 设置 `EXCALIBUR_SQL_TEST_DATABASE_URL` 后强制覆盖 SQL contract。
- org/project/device/action/audit 数据重启后保留。
- session rotation、refresh token reuse detection、API key revoke/scope 在 memory + SQL contract 中覆盖。

## Milestone 2: 真实 PKI 与 MQTT runtime

状态：CSR/dev-auth 已签发真实可解析 X.509 设备证书，fingerprint 从证书 DER 计算并持久化；store 已支持 active fingerprint 到 device identity lookup；rumqttd runtime 已支持 TLS listener，TLS peer certificate fingerprint 传入 auth handler；vendored rumqttd patch 已把 source client id 暴露给 runtime，并新增 publish/subscribe auth hook，使 publish/subscribe ACL 绑定到连接设备身份。明文 dev 模式可显式开启 username-as-fingerprint 过渡。

目标：

- 将 mTLS simulator 端到端矩阵纳入 CI。

验收：

- mTLS device-agent/simulator 可连接。
- revoked/expired/disabled cert connect 失败。
- cross project publish/subscribe 被拒绝。
- invalid payload 不写入。

## Milestone 3: Telemetry 写入链路

状态：MQTT ingest 可将 telemetry envelope 发布到 NATS-backed JetStream stream；worker 会幂等确保 stream 和 durable push consumer，按 batch 写入 store，并在成功写库后 ack；invalid envelope 会 dead-letter 并 ack。仍需引入官方 `async-nats` 或等价客户端替代当前 raw `nats-lite` MVP，并补 live NATS 集成测试。

目标：

- 将 MQTT QoS1 ACK 与 durable ingest/outbox 绑定，避免 broker ack 后 JetStream/storage 失败导致 telemetry 丢失。
- 官方 JetStream client 或完整 raw protocol 覆盖。
- Timescale COPY 或 batch insert。
- Duplicate sequence 策略。
- Continuous aggregates。

验收：

- telemetry p95 达标。
- NATS lag 可观测。
- retention/compression policy 生效。
- Dashboard query 不受 raw 写入明显拖慢。

## Milestone 4: Actions 与 OTA

状态：action target 持久状态、queued target claim、worker dispatcher、NATS command bus、MQTT command bridge、firmware metadata、短 TTL signed upload URL、approval/retry/cancel API 和 worker timeout sweeper 已接入。

目标：

- 将 action command bus 切到 JetStream durable subject/consumer，bridge 成功发布到 MQTT 后再 ack，覆盖 bridge restart/offline retry。
- action payload 只持久化 firmware/session 引用，worker dispatch 前即时签发短 TTL object URL，避免审批/排队期间 URL 过期。
- 直接对象存储 upload finalize/verify 流程。
- OTA rollout cohort、审批策略和回滚策略。
- Agent installer contract。

验收：

- 批量 OTA 状态可追踪。
- checksum/signature 错误失败。
- timeout/retry 行为稳定。
- audit 覆盖所有危险操作。

## Milestone 5: Dashboard、Alerts、Diagnostics

状态：Dashboard telemetry aggregate query API 已接入；alert worker 已支持 offline、threshold、window aggregation 扫描，写入 firing/resolved events 并记录 notification attempts；diagnostics session 已支持创建、短 TTL upload/download URL、finalize checksum/size 和 audit。

剩余目标：

- CSV/Parquet export。
- Email/webhook notification provider 从 NATS notification subject 接真实 provider。
- Diagnostics retention/lifecycle policy。

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
- MVP 首页已改为 API-backed 数据；生产化仍需拆分真实二级页面。
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

状态：API `/ready` 和 `/metrics` baseline、auth endpoint in-memory rate limit、Helm/Compose rate/alert/object storage env、Go/No-Go checklist 和 `scripts/api-load-smoke.sh` 已接入。

剩余目标：

- 完整 Prometheus metrics/log correlation/traces。
- Backup/restore 自动化和恢复演练产物。
- Secret management 接 ExternalSecret/KMS。
- 10 万设备 MQTT mTLS load test。
- Helm production values 按压测结果调优。

验收：

- 10 万设备目标压测报告。
- 故障演练报告。
- Runbook 完整。
- Go/No-Go checklist 可执行。
