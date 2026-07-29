# 设备协议

Excalibur 设备协议是平台原生协议，不兼容 Bytebeam。协议版本通过 MQTT topic 第一段 `v1` 表示。

## Topic

| 方向 | Topic | Payload |
| --- | --- | --- |
| Device publish | `v1/p/{project_id}/d/{device_id}/telemetry/{stream}` | JSON array。 |
| Device publish | `v1/p/{project_id}/d/{device_id}/shadow` | 最新 shadow JSON object。 |
| Device subscribe | `v1/p/{project_id}/d/{device_id}/commands` | 单个 command JSON object。 |
| Device publish | `v1/p/{project_id}/d/{device_id}/commands/status` | command status JSON array。 |

Topic segment 约束：

- `project_id` 和 `device_id` 在后端 parser 中按 UUID 解析。
- `stream` 必须非空。
- 设备只能访问自己证书绑定的 project/device topic。
- 不允许 `/tenants/...` topic。

## Telemetry payload

Telemetry payload 必须是 JSON array：

```json
[
  {
    "sequence": 42,
    "timestamp": "2026-07-29T08:30:00Z",
    "temperature": 24.6,
    "status": "ok"
  },
  {
    "sequence": 43,
    "timestamp": 1785313800000,
    "temperature": 24.8,
    "status": "ok"
  }
]
```

字段规则：

- `sequence` 必须是 integer。
- `timestamp` 可以是 RFC3339 string 或 Unix milliseconds。
- `sequence` 和 `timestamp` 会从 payload fields 中移除，分别写入列。
- 其余字段作为 JSONB payload 保存。

## Shadow payload

Shadow payload 是最新 shadow object，不是 array：

```json
{
  "firmware": {
    "motor": "3.2.1",
    "rootfs": "11.4.2"
  },
  "network": {
    "rssi": -58,
    "carrier": "private-5g"
  },
  "health": "nominal"
}
```

Agent serializer 对 shadow stream 取 buffer 中最后一个 object 发布，服务端更新 `devices.latest_shadow`。

## Command payload

Command 是单个 JSON object：

```json
{
  "action_id": "018f4c5c-9b4d-7cc2-a62a-44590f671301",
  "name": "ota.install",
  "payload": {
    "firmware_id": "018f4c5c-9b4d-7cc2-a62a-44590f671201",
    "component": "motor",
    "version": "3.2.1",
    "signed_url": "https://objects.example/firmware/motor-3.2.1.bin?sig=...",
    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "signature": "ed25519:optional-signature",
    "size_bytes": 1048576
  }
}
```

重要差异：

- `action_id` 是 UUID string。
- `payload` 是 JSON value，不是 stringified JSON。
- `name` 是 action 类型，例如 `ota.install`、`diagnostics.collect`。

## Command status payload

设备向 `commands/status` 发布 JSON array：

```json
[
  {
    "action_id": "018f4c5c-9b4d-7cc2-a62a-44590f671301",
    "state": "Running",
    "progress": 42,
    "errors": []
  }
]
```

允许状态：

- `Running`
- `Completed`
- `Failed`
- `Cancelled`
- `TimedOut`

后端当前也接受小写终态并映射；agent 端会把内部 `Downloaded` 归一为 `Completed`，其他非终态归一为 `Running`。

## Auth JSON

设备启动时读取 auth JSON：

```json
{
  "broker": "mqtt.local.excalibur.dev",
  "port": 8883,
  "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
  "device_id": "018f4c5c-9b4d-7cc2-a62a-44590f671101",
  "authentication": {
    "ca_certificate": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----",
    "device_certificate": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----",
    "device_private_key_path": "/etc/excalibur/device.key"
  },
  "provisioning_mode": "Csr",
  "production": true
}
```

`device-agent` 当前反序列化需要 `broker`、`port`、`project_id`、`device_id` 和可选 `authentication`。后端 `DeviceAgentAuthConfig` 还会返回 `provisioning_mode` 与 `production`，用于区分 CSR 生产路径和 dev-generated keypair 路径。

## 协议测试位置

- Rust 协议测试：`backend/crates/device-protocol/src/lib.rs`
- MQTT ingest ACL 测试：`backend/apps/mqtt-ingest/src/lib.rs`
- Agent serializer/action 测试：`device-agent/device-agent/src/base/serializer/mod.rs`、`device-agent/device-agent/src/base/actions.rs`
- Frontend topic helper 测试：`frontend/src/lib/protocol.test.ts`
