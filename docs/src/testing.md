# 测试策略

Excalibur 需要覆盖后端、协议、设备 agent、前端、部署和性能。当前 scaffold 已有单元和集成风格测试，生产化时应逐步扩展。

## 当前检查命令

后端：

```bash
cd backend
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo test --workspace
RUSTUP_TOOLCHAIN=stable cargo test --workspace --all-features
```

Device agent：

```bash
cd device-agent
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo test --workspace
```

Frontend：

```bash
cd frontend
bun run typecheck
bun run test
bun run build
```

Infra：

```bash
docker compose config
helm lint infra/helm/excalibur
```

Docs：

```bash
mdbook build docs
```

## Auth/RBAC 测试

需要覆盖：

- 注册密码长度限制。
- 登录成功和失败。
- 不存在用户返回通用错误。
- 未带 Bearer token 拒绝 protected routes。
- 非成员访问 project 被拒绝。
- 低权限角色创建/修改资源被拒绝。
- audit log 写入关键操作。
- API key scope 和 hash 存储。
- refresh token rotation 和 reuse detection。

当前已覆盖部分：短密码拒绝、缺 token、缺用户登录、跨 project access。

## MQTT security 测试

需要覆盖：

- 设备证书撤销后 connect 失败。
- disabled device connect/publish/subscribe 被拒绝。
- 设备 publish 到其他 project/device topic 被拒绝。
- 设备 subscribe 其他 device commands 被拒绝。
- 非 `v1` topic 被拒绝。
- UUID invalid topic 被拒绝。
- payload 非 array、缺 sequence、timestamp invalid 被拒绝。
- duplicate sequence 策略符合设计。

当前已覆盖 broker-agnostic ACL 和 command status ingest。

## Ingest integration 测试

目标场景：

1. 创建 org/project/device。
2. 设备 CSR provisioning。
3. mTLS connect。
4. 发布 telemetry。
5. 发布 shadow。
6. 发布 command status。
7. TimescaleDB 查询到数据。
8. Console 设备详情显示 latest shadow 和 last seen。

当前 HTTP `/api/v1/telemetry` 可以作为开发替身，但不能替代真实 MQTT integration 测试。

## Actions/OTA 测试

需要覆盖：

- `ota.install` payload validation。
- 创建 action target。
- dispatcher 发布 command。
- agent 接收 command。
- downloader 下载 signed URL。
- checksum 错误导致 failed。
- installer 成功导致 completed。
- timeout/retry/cancel 行为确定。
- 批量 OTA aggregate state 正确。

当前已覆盖 OTA payload validation 和 agent downloader 相关单元测试。

## Dashboard/alerts 测试

需要覆盖：

- 时间范围查询。
- stream/device filter。
- downsample 聚合。
- pagination。
- CSV export。
- offline alert 去重和恢复。
- threshold alert window。
- notification provider 成功/失败和 retry。

## Frontend E2E 测试

需要覆盖：

- 登录。
- 项目切换。
- 设备列表筛选。
- 设备详情。
- Dashboard panel 时间范围切换。
- 创建 action。
- OTA progress。
- RBAC 禁用按钮和拒绝路由。
- remote shell beta off 时入口 disabled。

当前前端主要是静态 scaffold 和 protocol helper tests。

## 性能测试

目标：

- 10 万设备连接。
- 多 stream publish。
- ingest batch writer p95。
- Dashboard query p95。
- Action fanout 吞吐。
- Reconnect storm 恢复。

建议工具：

- `examples/device-simulator` 扩展为 MQTT load simulator。
- NATS JetStream consumer lag exporter。
- Timescale query benchmark scripts。
- Frontend synthetic dashboard load。
