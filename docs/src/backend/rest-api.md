# REST API

REST API 前缀是 `/api/v1`。当前 API 使用 SQL-backed Bearer token session 认证：

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
| `POST` | `/api/v1/auth/register` | 注册用户，返回 access/refresh token，并设置 HttpOnly cookies。 |
| `POST` | `/api/v1/auth/login` | 登录，返回 access/refresh token，并设置 HttpOnly cookies。 |
| `POST` | `/api/v1/auth/refresh` | 轮换 refresh token，返回新的 access/refresh token，并刷新 HttpOnly cookies。 |
| `POST` | `/api/v1/auth/logout` | 撤销当前 session，并清理 HttpOnly cookies。 |

`RegisterRequest`：

```json
{
  "email": "ops@example.com",
  "password": "correct horse battery staple",
  "display_name": "Ops"
}
```

`AuthResponse`：

```json
{
  "token": "excs_...",
  "refresh_token": "excr_...",
  "expires_at": "2026-07-30T12:00:00Z",
  "refresh_expires_at": "2026-08-29T12:00:00Z",
  "user_id": "018f4c5c-9b4d-7cc2-a62a-44590f671010"
}
```

Refresh token 使用 rotation 语义：旧 refresh token 成功使用后会写入 reuse detection 表，再次使用会返回 unauthorized。Refresh 请求可以传 JSON `refresh_token`，也可以依赖 `excalibur_refresh` HttpOnly cookie。Logout 会撤销当前 access token 对应的 session，并清理 `excalibur_access` / `excalibur_refresh` cookies。

## API Keys

API key 用于自动化和服务端集成。当前管理接口仍要求用户 session，创建和撤销都会写 audit。资源 API 支持 `Authorization: Bearer excak_...` 或 `x-api-key`，并按 key 的 org/project scope 与字符串 scope enforcement 授权。

| 方法 | 路径 | 最小角色 | 说明 |
| --- | --- | --- | --- |
| `GET` | `/api/v1/api-keys?org_id=...&project_id=...` | Admin | 列出 org 或 project scope 下的 API keys，不返回明文 key。 |
| `POST` | `/api/v1/api-keys` | Admin | 创建 API key，明文 key 只在本次响应返回。 |
| `POST` | `/api/v1/api-keys/{api_key_id}/revoke?org_id=...` | Admin | 撤销 API key。 |

`CreateApiKeyRequest`：

```json
{
  "org_id": "018f4c5c-9b4d-7cc2-a62a-44590f671000",
  "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
  "name": "ci-ingest",
  "scopes": ["telemetry:write"],
  "expires_at": "2026-08-29T12:00:00Z"
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
| `GET` | `/api/v1/telemetry/aggregate?project_id=...&stream=...&field=...&bucket_seconds=60` | Viewer | Dashboard time-range bucket 聚合查询。 |
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
| `POST` | `/api/v1/firmware/{firmware_id}/upload-url` | Operator | 生成短 TTL upload signed URL。 |
| `POST` | `/api/v1/firmware/{firmware_id}/download-url` | Viewer | 生成短 TTL download signed URL。 |
| `POST` | `/api/v1/firmware/{firmware_id}/finalize` | Operator | 校验 size/checksum/signature 并标记 artifact verified。 |
| `POST` | `/api/v1/firmware/{firmware_id}/rollout` | Operator | 按 device_ids 或 cohort 创建 OTA rollout 和 action。 |
| `GET` | `/api/v1/firmware-rollouts?project_id=...` | Viewer | 列出 firmware rollout metadata。 |
| `GET` | `/api/v1/dashboards?project_id=...` | Viewer | 列出 dashboards。 |
| `POST` | `/api/v1/dashboards` | Operator | 创建 dashboard。 |
| `GET` | `/api/v1/alerts?project_id=...` | Viewer | 列出 alert rules。 |
| `POST` | `/api/v1/alerts` | Operator | 创建 alert rule。 |
| `GET` | `/api/v1/alert-events?project_id=...&state=Firing` | Viewer | 查询 alert firing/resolved events。 |
| `GET` | `/api/v1/diagnostics/sessions?project_id=...` | Viewer | 列出 diagnostics sessions。 |
| `POST` | `/api/v1/diagnostics/sessions` | Operator | 创建 diagnostics session、upload URL 和 action。 |
| `POST` | `/api/v1/diagnostics/sessions/{session_id}/finalize` | Operator | 写入 diagnostics object checksum/size。 |
| `POST` | `/api/v1/diagnostics/sessions/{session_id}/download-url` | Viewer | 生成 diagnostics download signed URL。 |
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
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "signature": "ed25519:optional-signature",
  "size_bytes": 1048576
}
```

API action payload 只保存 firmware reference 和校验 metadata；最终下发给设备的 `signed_url` 由 mqtt-ingest command bridge 发布 MQTT command 前即时签发。
