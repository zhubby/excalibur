# 身份、租户与 RBAC

Excalibur 的 SaaS 租户层级是：

```text
org -> project -> device
```

`org` 是商业租户和计费边界；`project` 是设备 fleet、stream、dashboard、firmware、alert 的隔离边界；`device` 是 MQTT 身份和遥测归属边界。

## 身份模型

当前领域模型：

| 模型 | 关键字段 |
| --- | --- |
| `User` | `email`、`display_name`、`password_hash`、`email_verified`。 |
| `Org` | `name`、`slug`。 |
| `Membership` | `org_id`、`user_id`、`role`。 |
| `Project` | `org_id`、`name`、`slug`。 |

当前 API：

- `POST /api/v1/auth/register` 使用 Argon2id 生成密码哈希，密码长度至少 12。
- `POST /api/v1/auth/login` 使用 Argon2id 校验密码，并用 dummy hash 避免明显用户枚举。
- 返回值包含 Bearer access token、refresh token 和过期时间，并同时设置 HttpOnly cookie。`memory` 模式把 session 存在进程内，`timescale` 模式只在 SQL 保存 access/refresh token hash。
- `POST /api/v1/auth/refresh` 使用 refresh token rotation，并记录已使用 refresh token hash 用于 reuse detection；请求可通过 JSON `refresh_token` 或 HttpOnly refresh cookie 完成。
- `POST /api/v1/auth/logout` 撤销当前 session，并清理 access/refresh cookie。
- API key 明文只在创建时返回一次，SQL/memory store 只保存 key hash、scope、expires/revoke/last_used 元数据。
- API key 可通过 `Authorization: Bearer excak_...` 或 `x-api-key` 认证资源 API，并按 org/project scope 与字符串 scope enforcement 授权；API key 管理接口仍要求用户 session。

生产目标：

- 邮箱验证。
- 邀请和 membership 管理。
- Session refresh、login failure 和异常登录 audit。

## RBAC 角色

角色从高到低：

| 角色 | 典型权限 |
| --- | --- |
| `Owner` | org 管理、计费、安全设置、所有资源。 |
| `Admin` | project 管理、成员管理、设备和规则配置。 |
| `Operator` | 创建设备、证书 provisioning、OTA、diagnostics、action 操作。 |
| `Viewer` | 只读查看 fleet、telemetry、dashboard、audit。 |

代码中的 `Role::permits(minimum)` 使用 rank 比较。handler 根据操作类型传入最小角色：

- 创建 project：`Admin`。
- 创建设备、stream、action、firmware、dashboard、alert：`Operator`。
- 查询 project、device、telemetry、actions、dashboard、alerts：`Viewer`。
- 查询 audit：org `Viewer` 起。

API key scope 支持精确 scope、`resource:*` wildcard 和全局 `*`。当前 handler 使用的 scope 包括：

- `projects:read` / `projects:write`
- `devices:read` / `devices:write` / `devices:provision`
- `streams:read` / `streams:write`
- `telemetry:read` / `telemetry:write`
- `actions:read` / `actions:write`
- `firmware:read` / `firmware:write`
- `dashboards:read` / `dashboards:write`
- `alerts:read` / `alerts:write`
- `audit:read`

## Tenant Context 要求

所有生产 repository 方法都必须显式接收 tenant context 或等效的 `org_id/project_id` 参数。禁止只用裸 `device_id`、`action_id`、`certificate_id` 查询并在应用层事后过滤。

推荐约束：

- 每个 project-scoped SQL 表都包含 `project_id`。
- 复合外键优先使用 `(project_id, id)`。
- 查询必须带 `WHERE project_id = $tenant_project_id`。
- MQTT connect 身份必须由证书 fingerprint 查出 `(project_id, device_id)`。
- MQTT publish/subscribe topic 中的 project/device 必须与认证身份完全一致。

## Audit

当前 API 已对部分敏感操作写 audit：

- `org.create`
- `project.create`
- `device.create`
- `device.csr_sign`
- `device.dev_auth_download`
- `device.certificate_revoke`
- `api_key.create`
- `api_key.revoke`

生产应继续覆盖：

- 登录、登出、session refresh。
- 邀请、角色变更、成员移除。
- OTA 创建、审批、取消。
- diagnostics session 创建、文件下载。
- remote shell open/close 和命令流元数据。
- alert rule 启停和 notification provider 变更。
