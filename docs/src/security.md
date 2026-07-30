# 安全模型

Excalibur 的安全边界覆盖人、设备、租户、数据、操作和运维。

## 资产

| 资产 | 风险 |
| --- | --- |
| 用户账号和 session | 控制面越权、凭证泄露。 |
| API keys | 自动化系统越权、长期凭证泄露。 |
| Device private key | 设备身份被克隆。 |
| Device certificate | 被撤销前可连接 broker。 |
| Firmware artifact | OTA 供应链攻击。 |
| Signed URL | 被滥用下载或上传对象。 |
| Remote shell session | 直接设备控制能力。 |
| Audit logs | 事后追踪和合规证据。 |

## 人类用户认证

当前实现：

- Argon2id 密码哈希。
- 登录对不存在用户使用 dummy password hash。
- SQL-backed Bearer token session，数据库只保存 access token hash。
- Refresh token rotation，旧 refresh token hash 会进入 reuse detection 表。
- Session revoke/logout。

生产要求：

- HttpOnly、Secure、SameSite cookie。
- MFA 预留。
- 邮箱验证和邀请。
- Login rate limit。
- 审计登录失败和异常登录。

## 设备认证

生产设备必须用 mTLS：

- 设备本地生成 private key。
- CSR 签发 device certificate。
- 平台保存 certificate fingerprint。
- Broker connect hook 以 fingerprint 找到 device。
- Revoked/expired/disabled 设备无法连接。

禁止：

- 生产使用 dev-auth inline private key。
- 在日志或 audit metadata 中写入 PEM 私钥。
- 多设备共享同一 certificate。

## Tenant isolation

每层都必须校验：

- API handler 校验 org/project role。
- Store/SQL repository 再次检查 project scope。
- MQTT publish/subscribe 校验证书身份与 topic 一致。
- Timescale 查询带 `project_id`。
- Object storage key prefix 包含 org/project 并由服务端签名，不直接暴露 bucket wide credential。

## API key

当前实现：

- 只在创建时返回明文。
- 数据库存 hash，不存明文。
- key 有 name、scope、expires_at、last_used_at。
- 支持 org/project scope。
- 支持 scopes 列表，由调用方按 `telemetry:write`、`automation:run` 等粒度扩展。
- 创建和撤销写 audit。

生产要求：

- 在需要自动化认证的 API/MQTT ingress 上接入 API key scope enforcement。
- API key request quota 和异常使用审计。
- 长期 key rotation/reminder。

## Remote shell

Remote shell 是高风险 beta。默认必须关闭。启用需要：

- `remote_shell` project feature flag。
- 专门 RBAC permission。
- 二次确认和目标设备明确展示。
- 短时 token 和 session TTL。
- WebSocket tunnel 服务端授权。
- 打开、关闭、命令流 metadata、错误都写 audit。
- 能从控制面强制关闭 session。
- 限制并发 session 和源 IP。

## OTA 安全

OTA 必须：

- Firmware artifact 保存 `sha256`、`size_bytes`、`signature`、`component`、`version`。
- Agent 下载后校验 hash。
- 生产应校验签名，并把 signer identity 写入 metadata。
- Signed URL TTL 短。
- Rollout 支持审批、暂停、取消和回滚。
- 安装脚本运行在最小权限环境。

## Secrets

本地 `.gitignore` 已覆盖：

- `*.pem`
- `*.key`
- `*.crt`
- `*.csr`
- `auth.json`
- `device.json`
- `dev-auth*.json`
- `device-agent/**/*auth*.json`

生产 secret 应使用：

- Kubernetes Secret 或 External Secrets。
- Cloud KMS/HSM 管理 CA key。
- Secret rotation playbook。

## Rate limit 与 abuse control

生产 API 和 MQTT 层应增加：

- Auth endpoints IP/user rate limit。
- API key request quota。
- MQTT connect rate limit。
- per-device publish rate limit。
- payload size limit。
- alert/diagnostics/OTA 操作频率限制。
