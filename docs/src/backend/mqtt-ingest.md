# MQTT Ingest Runtime

`backend/apps/mqtt-ingest` 是 MQTT 数据面的接入边界。当前代码把核心能力写成 broker-agnostic 函数，生产 `rumqttd` adapter 应在 connect、publish、subscribe hook 中调用这些函数。

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

生产 `rumqttd` connect hook 应根据 mTLS certificate fingerprint 查询出该结构。

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

## rumqttd adapter 目标

`rumqttd_adapter` 当前是 feature-gated placeholder。生产实现应提供：

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
