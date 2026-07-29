# 产品目标与边界

Excalibur 的目标是复刻 Bytebeam 类物联网平台的核心能力，但不复刻其协议兼容性。这里的“复刻”是能力层面的商业 SaaS 复刻：设备安全接入、遥测 Streams、Device Shadow、Actions/OTA、Dashboard、Alerts、Remote Diagnostics、RBAC、Audit、API Keys 和运维可观测性。

## 平台能力

| 能力 | 目标形态 | 当前仓库状态 |
| --- | --- | --- |
| 多租户 SaaS | `org -> project -> device`，所有 API、MQTT、查询都强制 project scope。 | domain/store/API 已按 org/project/device 建模。 |
| 设备 mTLS | 设备用证书连接 MQTT broker，证书 fingerprint 与 device/project 绑定。 | 证书模型、CSR/dev auth API scaffold 已有；真实 CA 签发和 broker connect hook 待实现。 |
| Telemetry Streams | 设备按 stream 批量发布 JSON array，TimescaleDB hypertable 存储。 | 协议解析、内存写入、Timescale migration 已有；批量 COPY 和 SQL repo 待实现。 |
| Device Shadow | 设备发布最新 shadow object，控制面维护 latest shadow。 | topic、agent serializer、API/mqtt ingest update 已有。 |
| Actions/Commands | 控制面创建 action，dispatcher 下发 command，设备回传状态。 | action 模型、payload 校验、status ingest 已有；dispatcher 发布链路待实现。 |
| OTA | 固件 artifact 保存在对象存储，`ota.install` command 下发 signed URL 和校验信息。 | payload 类型和 agent 下载校验已接入；S3 signer、审批、批量状态待实现。 |
| Alerts | offline、threshold、window aggregation 规则。 | alert rule 模型/API scaffold 已有；worker 扫描和通知待实现。 |
| Diagnostics | 日志、system stats、文件采集、导出。 | agent 日志/system stats stream 已有；诊断 session、对象存储上传待实现。 |
| Remote Shell | 高风险 beta 能力，必须 RBAC、短时授权、审计和 feature flag。 | agent 保留 gated 代码；API 默认拒绝 `remote_shell.open`。 |
| Console | 操作员 Web Console 管理 fleet、streams、actions、firmware、alerts、安全。 | 首页 scaffold 已实现静态数据和协议展示。 |

## 非目标

- 不兼容 Bytebeam MQTT topic、device config、action wire shape 或 REST API。
- 不在遥测路径上通过 Toasty 抽象 TimescaleDB hypertable、compression、retention 或 COPY。
- 不把 device-agent 合并进后端 Rust workspace。设备端保持独立 workspace，避免上游衍生依赖影响后端构建。
- 首阶段 Linux-first。Android 代码可以保留，但不是第一阶段验收条件。
- Remote shell 默认关闭，不能作为普通 action 无条件启用。

## 技术栈决策

- 后端控制面：Rust、`axum`、`utoipa`、`tower-http`。
- 控制面强模型边界：Toasty-ready，但当前生产 SQL repo 尚未接入。
- MQTT broker/runtime：`rumqttd` 作为目标运行时，当前 ACL/ingest 逻辑保持 broker-agnostic。
- 数据库：TimescaleDB，普通 PostgreSQL 表承载控制面，hypertable 承载遥测。
- 异步缓冲：NATS JetStream 作为 MQTT ingest 与 worker 间的推荐缓冲层。
- 对象存储：S3-compatible，用于 firmware artifact、diagnostics 文件、导出文件。
- 前端：Bun、TypeScript、Next.js App Router、Tailwind、lucide、TanStack Table、uPlot。

## 规模假设

第一阶段按 10 万设备级商业 MVP 设计。当前代码是可运行 scaffold，不是 10 万设备生产容量的完成实现。达到该目标需要补齐：

- SQL repositories 和连接池治理。
- MQTT mTLS connect/publish/subscribe hook。
- NATS JetStream buffer 和 worker dispatcher。
- Timescale COPY batch ingest、continuous aggregate、retention、compression 验证。
- Dashboard query cache 和 pagination/export。
- 生产级密钥、证书、审计、告警、备份和压测体系。
