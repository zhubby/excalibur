# 术语表

| 术语 | 说明 |
| --- | --- |
| Org | 商业租户边界，包含成员和 projects。 |
| Project | 设备、stream、dashboard、firmware、alert 的隔离边界。 |
| Device | 物理或虚拟设备，绑定 project 和证书。 |
| Stream | 一类遥测数据，例如 `battery`、`motor`、`logs`。 |
| Telemetry | 设备发布的时间序列数据。 |
| Shadow | 设备最新状态快照，发布到专用 shadow topic。 |
| Action | 平台下发给设备的命令，例如 OTA 或 diagnostics。 |
| Command | MQTT wire 层的 action 表达。 |
| Command status | 设备回传 action 执行进度和终态。 |
| OTA | Over-the-air update，远程固件或软件更新。 |
| CSR | Certificate Signing Request，设备本地生成私钥后提交的证书签名请求。 |
| mTLS | Mutual TLS，服务端和客户端都通过证书认证。 |
| Fingerprint | 证书 SHA-256 摘要，用于把证书绑定到 device。 |
| TimescaleDB | PostgreSQL 扩展，提供 hypertable、compression、retention 等时序能力。 |
| Hypertable | TimescaleDB 的时序分区表。 |
| NATS JetStream | 推荐的 ingest/worker 缓冲和事件流。 |
| S3-compatible storage | MinIO、AWS S3 等对象存储，用于 firmware、diagnostics、export。 |
| RBAC | Role-based access control，基于角色的访问控制。 |
| Audit log | 安全敏感操作的追加日志。 |
| Remote shell | 高风险 beta 能力，通过短时授权打开设备 shell tunnel。 |
