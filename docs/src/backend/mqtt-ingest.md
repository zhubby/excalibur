# MQTT Ingest Runtime

`backend/apps/mqtt-ingest` 是 MQTT 数据面的接入边界。它包含 broker-agnostic ACL/ingest 函数，也包含一个可运行的本地 `rumqttd` runtime。当前 runtime 监听 MQTT v4，订阅 Excalibur native publish topic，并把 telemetry、shadow 和 command status 写入 `Store`。

## 当前库接口

核心类型：

- `AuthenticatedDevice`
- `IngestError`
- `authorize_publish`
- `authorize_subscribe`
- `ingest_publish`

`AuthenticatedDevice` 包含：

```text
pub struct AuthenticatedDevice {
    pub project_id: Id,
    pub device_id: Id,
    pub status: DeviceStatus,
}
```

本地 runtime 会从 topic 中解析 `(project_id, device_id)`，查询 device 状态后调用 `ingest_publish`。生产 `rumqttd` connect hook/fork 还应根据 mTLS certificate fingerprint 查询出该结构，并把连接身份绑定到后续 publish/subscribe ACL。

## 本地 runtime

启动：

```bash
make mqtt
```

默认配置：

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `MQTT_LISTEN` | `0.0.0.0:1883` | rumqttd MQTT v4 listener。 |
| `MQTT_MAX_CONNECTIONS` | `10000` | router 最大连接数。 |
| `MQTT_MAX_PAYLOAD_SIZE` | `262144` | 单 publish payload 上限。 |
| `MQTT_MAX_INFLIGHT_COUNT` | `100` | 单连接 inflight 上限。 |
| `MQTT_CONNECTION_TIMEOUT_MS` | `60000` | MQTT CONNECT 等待超时。 |
| `STORAGE_BACKEND` | 根据 `DATABASE_URL` 自动选择 | `DATABASE_URL` 存在时默认 `timescale`，否则 `memory`。 |
| `DATABASE_URL` | 无 | SQL-backed ingest 的 TimescaleDB DSN。 |

runtime 内部订阅：

```text
v1/p/+/d/+/telemetry/+
v1/p/+/d/+/shadow
v1/p/+/d/+/commands/status
```

当前 runtime 是第一版可运行接入：它能让真实 MQTT publish 进入 Store，但不声称已完成生产级 per-connection ACL。rumqttd 0.20 公开 external auth hook 只返回 bool，没有把 peer certificate fingerprint 或连接身份暴露给 publish/subscribe hook；生产 mTLS 强校验需要补 hook 或维护 fork。

## Publish ACL

允许 publish 的 topic：

- `v1/p/{project_id}/d/{device_id}/telemetry/{stream}`
- `v1/p/{project_id}/d/{device_id}/shadow`
- `v1/p/{project_id}/d/{device_id}/commands/status`

拒绝条件：

- 设备状态是 `Disabled`。
- topic 不是 `v1` 协议。
- UUID 无法解析。
- topic 中 project/device 与认证身份不一致。
- payload shape 不符合该 topic 要求。

## Subscribe ACL

允许 subscribe 的 topic：

- `v1/p/{project_id}/d/{device_id}/commands`

设备不能订阅其他 device 或 project 的 command topic。

## Ingest 行为

| Topic 类型 | 处理 |
| --- | --- |
| Telemetry | payload 必须是 JSON array；每个 item 需要 `sequence` 和 `timestamp`；其余字段写入 payload。 |
| Shadow | payload 是最新 shadow object；更新 device `latest_shadow` 和 `last_seen_at`。 |
| Command status | payload 是 status JSON array；更新 action 状态和设备 heartbeat。 |

Telemetry record 支持两种 timestamp：

- Unix milliseconds number。
- RFC3339 string。

## rumqttd hardening 目标

生产实现应继续补齐：

1. TLS listener 配置和 CA trust。
2. connect hook：
   - 读取 peer certificate。
   - 计算 SHA-256 fingerprint。
   - 查询 active certificate。
   - 校验证书未过期、未撤销、设备未禁用。
   - 将 `(project_id, device_id)` 写入 connection context。
3. publish hook：
   - 调用 `authorize_publish`。
   - 对 telemetry 使用 batch writer 或 NATS JetStream。
   - 对 shadow/status 使用持久 store 更新。
4. subscribe hook：
   - 调用 `authorize_subscribe`。
5. disconnect hook：
   - 更新 presence 或等待 offline worker 判断。

## NATS JetStream 缓冲

建议在 ingest 和 worker 之间引入 JetStream：

- MQTT hook 不应被 TimescaleDB 慢写长期阻塞。
- 以 project/device/stream 分区设计 subject。
- worker 批量消费并写入 TimescaleDB。
- command dispatcher 也可通过 JetStream 接收 action queued 事件。

示例 subject 设计：

```text
ingest.telemetry.{project_id}.{stream}
ingest.shadow.{project_id}
actions.dispatch.{project_id}
actions.status.{project_id}
alerts.evaluate.{project_id}
```

## Backpressure 与幂等

生产 ingest 必须定义：

- MQTT QoS 策略，当前 agent 使用 QoS 1。
- duplicate sequence 策略，推荐对同一 `(project_id, device_id, stream, sequence, ts)` upsert-ignore。
- payload size limit，与 agent `max_packet_size` 对齐。
- NATS publish timeout 和 broker disconnect 策略。
- Timescale batch flush size、flush interval、retry 和 dead-letter subject。
