# 产品目标与边界

Excalibur 的目标是复刻 Bytebeam 类物联网平台的核心能力，但不复刻其协议兼容性。这里的“复刻”是能力层面的商业 SaaS 复刻：设备安全接入、遥测 Streams、Device Shadow、Actions/OTA、Dashboard、Alerts、Remote Diagnostics、RBAC、Audit、API Keys 和运维可观测性。

## 平台能力

| 能力 | 目标形态 | 当前仓库状态 |
| --- | --- | --- |
| 多租户 SaaS | `org -> project -> device`，所有 API、MQTT、查询都强制 project scope。 | domain/store/API 已按 org/project/device 建模。 |
| 设备 mTLS | 设备用证书连接 MQTT broker，证书 fingerprint 与 device/project 绑定。 | CSR/dev-auth 已签发真实证书并持久化 fingerprint；runtime 支持 TLS listener、peer cert fingerprint 身份校验、publish/subscribe ACL；username-as-fingerprint 仅作显式 dev 过渡。 |
| Telemetry Streams | 设备按 stream 批量发布 JSON array，TimescaleDB hypertable 存储。 | 协议解析、JetStream ingest buffer、远端 publish durable ack gate、worker batch writer、Timescale migration 和 SQL repository 写入/查询已实现；高吞吐 COPY writer 和本地 disk outbox 待生产化。 |
| Device Shadow | 设备发布最新 shadow object，控制面维护 latest shadow。 | topic、agent serializer、API/mqtt ingest update 已有。 |
| Actions/Commands | 控制面创建 action，dispatcher 下发 command，设备回传状态。 | action 模型、payload 校验、status ingest、worker dispatcher、NATS command bus 和 MQTT command bridge 已接入。 |
| OTA | 固件 artifact 保存在对象存储，`ota.install` command 下发 signed URL 和校验信息。 | payload 类型、agent 下载校验、firmware metadata、signed upload/download URL、finalize 校验、rollout cohort、approval/retry/cancel/timeout 状态转换已接入；完整 rollback 自动化待实现。 |
| Alerts | offline、threshold、window aggregation 规则。 | alert rule API、worker 扫描、firing/resolved event 去重恢复和 notification attempt 计数已接入；真实 email/webhook provider 待实现。 |
| Diagnostics | 日志、system stats、文件采集、导出。 | diagnostics session、短 TTL upload/download URL、object finalize、action dispatch 和 audit 已接入；retention/lifecycle policy 待实现。 |
| Remote Shell | 高风险 beta 能力，必须 RBAC、短时授权、审计和 feature flag。 | agent 保留 gated 代码；API 默认拒绝 `remote_shell.open`。 |
| Console | 操作员 Web Console 管理 fleet、streams、actions、firmware、alerts、安全。 | 首页已 API-backed，TS DTO 从 OpenAPI schema 生成；真实二级页面和 E2E 仍需补齐。 |

## 非目标

- 不兼容 Bytebeam MQTT topic、device config、action wire shape 或 REST API。
- 不在遥测路径上通过 Toasty 抽象 TimescaleDB hypertable、compression、retention 或 COPY。
- 不把 device-agent 合并进后端 Rust workspace。设备端保持独立 workspace，避免上游衍生依赖影响后端构建。
- 首阶段 Linux-first。Android 代码可以保留，但不是第一阶段验收条件。
- Remote shell 默认关闭，不能作为普通 action 无条件启用。

## 技术栈决策

- 后端控制面：Rust、`axum`、`utoipa`、`tower-http`。
- 控制面强模型边界：Toasty-ready；当前 repository 先用 SQLx raw queries 覆盖控制面和 telemetry。
- MQTT broker/runtime：`rumqttd` 本地 runtime 已接入，核心 ACL/ingest 逻辑仍保持 broker-agnostic。
- 数据库：TimescaleDB，普通 PostgreSQL 表承载控制面，hypertable 承载遥测。
- 异步缓冲：NATS JetStream 作为 MQTT ingest 与 worker 间的推荐缓冲层。
- 对象存储：S3-compatible，用于 firmware artifact、diagnostics 文件、导出文件。
- 前端：Bun、TypeScript、Next.js App Router、Tailwind、lucide、TanStack Table、uPlot。

## 规模假设

第一阶段按 10 万设备级商业 MVP 设计。当前代码是可运行 scaffold，不是 10 万设备生产容量的完成实现。达到该目标需要补齐：

- 连接池治理、migration rollback/lock/audit 和生产 schema 演进流程。
- mTLS simulator 矩阵和大规模 TLS 连接压测。
- NATS JetStream buffer 和 worker dispatcher 的 live NATS/scale 验证。
- Timescale COPY batch ingest、continuous aggregate、retention、compression 压测验证。
- Dashboard query cache 和 pagination/export。
- 生产级密钥、证书、审计、告警、备份和压测体系。
