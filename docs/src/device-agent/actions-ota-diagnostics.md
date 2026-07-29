# Actions、OTA 与诊断

Excalibur 使用 action 模型表达平台到设备的命令。设备订阅 commands topic，执行后持续发布 command status。

## Command shape

```json
{
  "action_id": "018f4c5c-9b4d-7cc2-a62a-44590f671301",
  "name": "diagnostics.collect",
  "payload": {
    "session_id": "018f4c5c-9b4d-7cc2-a62a-44590f671401",
    "paths": ["/var/log/app"],
    "include_logs": true,
    "include_system_stats": true,
    "upload_url": "https://objects.example/diagnostics/session.tar.zst?sig=..."
  }
}
```

`payload` 是 JSON value。Agent 内部可以通过 `payload_as<T>()` 反序列化，也可以通过 `payload_string()` 兼容需要字符串参数的旧 collector。

## Status shape

```json
[
  {
    "action_id": "018f4c5c-9b4d-7cc2-a62a-44590f671301",
    "state": "Running",
    "progress": 45,
    "errors": []
  }
]
```

终态：

- `Completed`
- `Failed`
- `Cancelled`
- `TimedOut`

Agent 会将 `Downloaded` 映射为 `Completed`，其他执行中状态映射为 `Running`。

## OTA: `ota.install`

API 创建 action 时会校验 `OtaInstallPayload`：

```json
{
  "firmware_id": "018f4c5c-9b4d-7cc2-a62a-44590f671201",
  "component": "motor",
  "version": "3.2.1",
  "signed_url": "https://objects.example/firmware/motor-3.2.1.bin?sig=...",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "signature": "ed25519:optional-signature",
  "size_bytes": 1048576
}
```

校验规则：

- `component` 非空。
- `version` 非空。
- `signed_url` 是 `http://` 或 `https://` absolute URL。
- `sha256` 是 64 位 hex。
- `size_bytes` 大于 0。

Agent downloader 支持：

- 使用设备证书下载 HTTPS 资源。
- 读取 `signed_url`。
- 校验 `sha256`。
- 识别 `size_bytes`。
- 下载进度 status。
- 下载完成后把 action 交给 installer 或 app route。

生产 OTA 还需要：

- 固件上传 API。
- S3 signed URL 生成。
- artifact signature 校验。
- rollout cohort 和审批。
- per-target action 状态。
- timeout/retry/cancel 策略。
- 回滚策略和安全启动链路。

## Diagnostics: `diagnostics.collect`

Payload：

```json
{
  "session_id": "018f4c5c-9b4d-7cc2-a62a-44590f671401",
  "paths": ["/var/log/app", "/etc/app/config.json"],
  "include_logs": true,
  "include_system_stats": true,
  "upload_url": "https://objects.example/diagnostics/session.tar.zst?sig=..."
}
```

生产目标：

- API 创建 diagnostics session。
- Worker 生成短时 upload URL。
- Agent 打包受允许路径。
- Agent 上传文件到 S3-compatible storage。
- API 记录 session state 和 audit。
- Console 展示下载链接和过期时间。

## Remote shell: `remote_shell.open`

Payload 类型：

```json
{
  "session_id": "018f4c5c-9b4d-7cc2-a62a-44590f671501",
  "websocket_url": "wss://api.example.com/device-shell/session",
  "expires_at": "2026-07-29T08:45:00Z"
}
```

当前 API 默认拒绝：

```text
remote shell beta is disabled for this project
```

生产启用前必须满足：

- 默认关闭。
- project beta flag。
- 显式 RBAC permission。
- 短时授权。
- session TTL。
- 全量 audit。
- 服务端 WebSocket tunnel。
- 命令流元数据审计。
- 可快速 kill session。

现有 Tunshell 代码只能作为可选 provider，不应作为默认生产方案。
