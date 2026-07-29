# REST API

REST API 前缀是 `/api/v1`。当前 API 使用 Bearer token 认证：

```http
Authorization: Bearer <token>
```

OpenAPI 可从以下地址获取：

```text
GET /api/v1/openapi.json
```

## 通用规则

- project-scoped 读接口通常通过 query string 传入 `project_id`。
- org-scoped 读接口通常通过 query string 传入 `org_id`。
- 创建接口的 request body 通常包含 `project_id` 或 `org_id`。
- API 返回 JSON。
- 错误响应形态：

```json
{
  "error": "tenant scope violation"
}
```

## Auth

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `POST` | `/api/v1/auth/register` | 注册用户并返回 token。 |
| `POST` | `/api/v1/auth/login` | 登录并返回 token。 |

`RegisterRequest`：

```json
{
  "email": "ops@example.com",
  "password": "correct horse battery staple",
  "display_name": "Ops"
}
```

## Orgs 与 Projects

| 方法 | 路径 | 最小角色 | 说明 |
| --- | --- | --- | --- |
| `GET` | `/api/v1/orgs` | 当前用户 | 列出用户所属 org。 |
| `POST` | `/api/v1/orgs` | 当前用户 | 创建 org，创建者成为 Owner。 |
| `GET` | `/api/v1/projects?org_id=...` | Viewer | 列出 org 下 project。 |
| `POST` | `/api/v1/projects` | Admin | 创建 project。 |

`CreateProjectRequest`：

```json
{
  "org_id": "018f4c5c-9b4d-7cc2-a62a-44590f671000",
  "name": "Factory EV Line",
  "slug": "factory-ev-line"
}
```

## Devices 与 Provisioning

| 方法 | 路径 | 最小角色 | 说明 |
| --- | --- | --- | --- |
| `GET` | `/api/v1/devices?project_id=...` | Viewer | 列出设备。 |
| `POST` | `/api/v1/devices` | Operator | 创建设备。 |
| `POST` | `/api/v1/devices/{device_id}/provision` | Operator | 兼容当前 scaffold 的 dev provisioning 入口。 |
| `POST` | `/api/v1/devices/{device_id}/provision/csr` | Operator | 生产路径，设备提交 CSR，服务端返回 cert 和 key path 配置。 |
| `POST` | `/api/v1/devices/{device_id}/provision/dev-auth` | Operator | 开发/批量实验路径，返回 inline private key。 |
| `POST` | `/api/v1/devices/{device_id}/certificates/{certificate_id}/revoke?project_id=...` | Operator | 撤销证书。 |

`CreateDeviceRequest`：

```json
{
  "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
  "name": "press-line-a-017",
  "metadata": {
    "line": "A",
    "site": "shanghai"
  }
}
```

`CsrProvisionRequest`：

```json
{
  "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
  "csr_pem": "-----BEGIN CERTIFICATE REQUEST-----\n...\n-----END CERTIFICATE REQUEST-----",
  "device_private_key_path": "/etc/excalibur/device.key"
}
```

## Streams 与 Telemetry

| 方法 | 路径 | 最小角色 | 说明 |
| --- | --- | --- | --- |
| `GET` | `/api/v1/streams?project_id=...` | Viewer | 列出 stream definitions。 |
| `POST` | `/api/v1/streams` | Operator | 创建 stream definition。 |
| `GET` | `/api/v1/telemetry?project_id=...&device_id=...&stream=...&limit=100` | Viewer | 查询遥测，limit 最大 1000。 |
| `POST` | `/api/v1/telemetry` | Operator | 开发用 HTTP ingest，接受 MQTT topic 和 payload。 |

`CreateStreamRequest`：

```json
{
  "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
  "name": "battery",
  "fields": [
    { "name": "voltage", "field_type": "Float", "required": true },
    { "name": "temperature", "field_type": "Float", "required": false }
  ]
}
```

## Actions、Firmware、Alerts、Dashboards

| 方法 | 路径 | 最小角色 | 说明 |
| --- | --- | --- | --- |
| `GET` | `/api/v1/actions?project_id=...` | Viewer | 列出 actions。 |
| `POST` | `/api/v1/actions` | Operator | 创建 device action。 |
| `POST` | `/api/v1/actions/{action_id}/status` | Operator | 开发/控制面状态更新入口。 |
| `GET` | `/api/v1/firmware?project_id=...` | Viewer | 列出 firmware artifacts。 |
| `POST` | `/api/v1/firmware` | Operator | 创建 firmware metadata。 |
| `GET` | `/api/v1/dashboards?project_id=...` | Viewer | 列出 dashboards。 |
| `POST` | `/api/v1/dashboards` | Operator | 创建 dashboard。 |
| `GET` | `/api/v1/alerts?project_id=...` | Viewer | 列出 alert rules。 |
| `POST` | `/api/v1/alerts` | Operator | 创建 alert rule。 |
| `GET` | `/api/v1/audit?org_id=...&project_id=...` | Viewer | 查询 audit logs。 |

支持的 action 名称：

- `ota.install`
- `diagnostics.collect`
- `remote_shell.open`，当前默认拒绝，返回 beta disabled。

`ota.install` payload 必须包含：

```json
{
  "firmware_id": "018f4c5c-9b4d-7cc2-a62a-44590f671201",
  "component": "motor",
  "version": "3.2.1",
  "signed_url": "https://objects.example/firmware.bin?signature=...",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "signature": "ed25519:optional-signature",
  "size_bytes": 1048576
}
```
