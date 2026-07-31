# MQTT Ingest Runtime

`backend/apps/mqtt-ingest` 是 MQTT 数据面的接入边界。它包含 broker-agnostic ACL/ingest 函数，也包含一个可运行的本地 `rumqttd` runtime。当前 runtime 监听 MQTT v4，订阅 Excalibur native publish topic，把 shadow 和 command status 写入 `Store`，并可将 telemetry 先写入 NATS JetStream buffer，由 worker 批量落库。

## 当前库接口

核心类型：

- `AuthenticatedDevice`
- `IngestError`
- `authorize_publish`
- `authorize_subscribe`
- `ingest_publish`
- `authenticate_device_certificate_fingerprint`
- `telemetry_envelope_from_publish`
- `write_telemetry_envelope`

`AuthenticatedDevice` 包含：

```text
pub struct AuthenticatedDevice {
    pub project_id: Id,
    pub device_id: Id,
    pub status: DeviceStatus,
}
```

本地 runtime 默认仍可在 dev 模式下从 topic 中解析 `(project_id, device_id)`。开启 `MQTT_REQUIRE_CERT_FINGERPRINT_USERNAME=true` 后，runtime 要求设备使用稳定非空 ClientId，并会把 certificate fingerprint 查询为 active device，记录 `client_id -> device identity`，再用 vendored rumqttd patch 暴露的 source client id 校验 publish/subscribe topic。TLS listener 可直接从 peer certificate DER 计算 fingerprint；本地明文 dev 模式可显式允许 username-as-fingerprint 过渡。Telemetry 可直接写 Store，也可先发布为 NATS JetStream envelope。

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
| `MQTT_TLS_CA_CERT_PATH` | 无 | 配置后启用 TLS listener 的 CA trust。 |
| `MQTT_TLS_SERVER_CERT_PATH` | 无 | TLS server certificate path。 |
| `MQTT_TLS_SERVER_KEY_PATH` | 无 | TLS server private key path。 |
| `MQTT_REQUIRE_CERT_FINGERPRINT_USERNAME` | `false` | 开启后要求连接身份绑定 certificate fingerprint；TLS 模式从 peer cert 读取，明文 dev 模式可配合 `MQTT_ALLOW_PLAINTEXT_FINGERPRINT_AUTH=true` 使用 username 过渡。 |
| `MQTT_ALLOW_PLAINTEXT_FINGERPRINT_AUTH` | `false` | 仅本地开发用，允许无 TLS 时 username-as-fingerprint。 |
| `MQTT_TELEMETRY_BUFFER` | `auto` | `auto`/`direct`/`nats`；`auto` 在有 `NATS_URL` 时使用 NATS。 |
| `MQTT_TELEMETRY_NATS_SUBJECT` | `excalibur.telemetry.ingest` | telemetry envelope publish subject。 |
| `MQTT_TELEMETRY_NATS_STREAM` | `EXCALIBUR_TELEMETRY` | JetStream stream name。 |
| `MQTT_TELEMETRY_DEAD_LETTER_SUBJECT` | `excalibur.telemetry.dead_letter` | JetStream stream dead-letter subject，需与 worker telemetry dead-letter subject 一致。 |
| `MQTT_COMMAND_BRIDGE` | `auto` | `auto`/`disabled`/`nats`；有 NATS 时订阅 command bus 并发布到本地 broker。 |
| `MQTT_COMMAND_NATS_SUBJECT` | `excalibur.commands.dispatch` | worker action dispatcher 发布的 command envelope subject。 |
| `MQTT_COMMAND_NATS_STREAM` | `EXCALIBUR_COMMANDS` | command bridge 使用的 JetStream stream name。 |
| `MQTT_COMMAND_DELIVERY_SUBJECT` | `excalibur.commands.deliver` | command bridge durable push consumer delivery subject。 |
| `MQTT_COMMAND_DURABLE` | `excalibur-mqtt-command-bridge` | command bridge durable consumer name。 |
| `MQTT_COMMAND_QUEUE_GROUP` | 同 `MQTT_COMMAND_DURABLE` | command bridge queue group。 |
| `MQTT_COMMAND_DEAD_LETTER_SUBJECT` | `excalibur.commands.dead_letter` | invalid command envelope dead-letter subject。 |
| `MQTT_COMMAND_DOWNLOAD_URL_TTL_SECONDS` | `900` | `ota.install` publish 到 MQTT broker 前即时签发 download URL 的 TTL。 |
| `STORAGE_BACKEND` | 根据 `DATABASE_URL` 自动选择 | `DATABASE_URL` 存在时默认 `timescale`，否则 `memory`。 |
| `DATABASE_URL` | 无 | SQL-backed ingest 的 TimescaleDB DSN。 |
| `NATS_URL` | 无 | NATS DSN。 |

runtime 内部订阅：

```text
v1/p/+/d/+/telemetry/+
v1/p/+/d/+/shadow
v1/p/+/d/+/commands/status
```

当前 runtime 能让真实 MQTT publish 进入 Store 或 JetStream，并能把 worker command envelope 转发到设备 commands topic。仓库通过本地 vendored rumqttd patch 在 `Forward` 中携带 source client id，并新增 publish/subscribe authorization hook、publish ack gate、effective ClientId 传递和 TLS peer fingerprint 传递；因此开启 `MQTT_REQUIRE_CERT_FINGERPRINT_USERNAME=true` 时，cross-device/cross-project publish 和 commands subscribe 会按连接身份拒绝。NATS telemetry buffer 模式下，远端 telemetry publish 会先完成 topic/payload 校验和 JetStream PubAck，再交给 router 生成 MQTT QoS1 PUBACK；失败时 publish 不会被 router ack。

NATS command bridge 模式下，mqtt-ingest 会幂等确保 `EXCALIBUR_COMMANDS` stream 和 durable push consumer。worker 先把 reference-only command envelope 写入 JetStream；bridge 从 delivery subject 消费，发布前确认 action target 仍是 `Running`。`ota.install` 会在 bridge 发布到本地 broker 前即时查 firmware metadata 并签发短 TTL download URL，因此 action DB 和 JetStream command stream 不持久化 signed URL。只有成功发布到本地 MQTT broker 后才 ack JetStream message；已取消/超时的 stale command 会直接 ack 丢弃；无效 command envelope 或永久无效 OTA metadata 会写入 dead-letter subject 后 ack，避免 poison message 阻塞；MQTT publish 失败不会 ack，等待 JetStream redelivery。

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

## rumqttd identity hardening

当前实现：

1. TLS listener 支持 CA trust、server cert 和 server key 配置。
2. connect auth handler 要求稳定非空 ClientId，并会用 TLS peer certificate fingerprint 查询 active certificate；明文 dev 模式只有显式开启 `MQTT_ALLOW_PLAINTEXT_FINGERPRINT_AUTH` 时才接受 username fingerprint。
3. runtime 维护 `client_id -> (project_id, device_id)` 连接身份，并在 publish 路径调用 `authorize_publish`。
4. vendored rumqttd patch 新增 publish/subscribe authorization hook 和 publish ack gate，commands topic 订阅会调用 `authorize_subscribe`。

剩余生产目标：

1. disconnect hook 更新 presence，或由 offline worker 统一判断。
2. mTLS simulator 矩阵纳入 CI。

## NATS JetStream 缓冲

ingest 和 worker 之间已接入第一版 JetStream buffer：

- MQTT hook 不应被 TimescaleDB 慢写长期阻塞。
- ingest 把 telemetry publish 编码为 `TelemetryIngestEnvelope`，NATS 模式下先拿到 JetStream PubAck，再允许 broker ACK 设备 publish。
- worker 幂等确保 stream 和 durable push consumer。
- worker 批量写入 TimescaleDB，并在成功写库后 ack。
- invalid envelope 会写入 dead-letter subject 并 ack，避免 poison message 无限重试。
- command dispatcher 通过 NATS command bus 交给 mqtt-ingest command bridge。

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
- NATS publish timeout、长连接 publisher、熔断和 broker disconnect 策略。
- Timescale batch flush size、flush interval、retry 和 dead-letter subject。
