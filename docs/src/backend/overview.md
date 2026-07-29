# 后端总览

后端采用 Rust workspace，目标是把控制面强模型、设备协议、MQTT ingest 和后台 worker 分清边界。

## Crates 与 apps

| 路径 | 职责 |
| --- | --- |
| `backend/apps/api` | Axum API，负责 auth、RBAC、tenant scope、OpenAPI、SSE 和控制面 handler。 |
| `backend/apps/mqtt-ingest` | 本地 `rumqttd` MQTT broker runtime，复用 broker-agnostic ACL 和 ingest 逻辑写入 Store。 |
| `backend/apps/worker` | 后台 worker scaffold，后续负责 alert scan、action timeout、dispatcher、retention/export 等任务。 |
| `backend/crates/domain` | 领域模型，包含 `User`、`Org`、`Project`、`Device`、`DeviceCertificate`、`StreamDefinition`、`Action`、`FirmwareArtifact`、`AlertRule`、`Dashboard`、`AuditLog`。 |
| `backend/crates/device-protocol` | 设备 wire contract，包含 MQTT topic helper、parser、payload decoder、device auth JSON 和 action payload 类型。 |
| `backend/crates/storage` | 提供 `MemoryStore`、`PgStore` 和统一 `Store` 包装层，并保留 Toasty integration boundary。 |

## 请求处理原则

所有 handler 都遵循以下原则：

1. 从 `Authorization: Bearer <token>` 解析 actor。
2. 对 org/project scope 做显式权限检查。
3. 对输入 payload 做最小必要验证。
4. 调用 store 层，store 层再次检查 project ownership。
5. 对安全敏感动作写 audit log。

当前 API 的 session 保存在进程内 `HashMap`。这只适合开发验证；生产需要 HttpOnly cookie session、refresh token rotation、持久 session 表、设备 API key hash、邮箱验证和邀请流程。

## OpenAPI

API 使用 `utoipa` 生成 OpenAPI：

```text
GET /api/v1/openapi.json
```

后续前端应基于该 OpenAPI 生成 TS client，避免手写 REST 类型漂移。

## Health 与 Readiness

- `/health` 是轻量 liveness endpoint，只表示 API 进程可响应。
- `/ready` 会调用当前 `Store` 的 readiness check；`timescale` 模式会执行数据库 `SELECT 1`。Helm readiness probe 指向 `/ready`。

## 当前运行模式

API main 会读取 `STORAGE_BACKEND`：

| 值 | 行为 |
| --- | --- |
| `memory` | 正常启动，使用内存开发 store。 |
| `timescale` | 使用 SQL repository，要求 `DATABASE_URL` 指向已初始化的 TimescaleDB schema；启动时执行 schema validation。 |
| 其他 | fail fast。 |

生产化前不应把 `memory` 模式暴露为真实 SaaS 环境。SQL repository 已覆盖控制面和 telemetry 表，但 session 仍在 API 进程内，生产需要迁移到持久 session/refresh-token 存储。

## 关键工程约束

- 遥测和 Timescale 特性不通过 Toasty 抽象。
- 控制面 repositories 可以在 `storage` 边界内替换为 Toasty/SQL 实现。
- MQTT ingest 必须保持 project/device 作用域校验；本地 runtime 可按 topic 查 device，生产 runtime 必须额外绑定连接证书身份。
- Worker 必须以幂等方式处理 action dispatcher、timeout、alert notification 和 export。
