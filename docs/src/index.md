# Excalibur 文档

Excalibur 是一个 Bytebeam-like 的商业物联网 SaaS 平台 scaffold。它采用 Rust `axum` 控制面、Toasty-ready 控制面模型边界、broker-agnostic MQTT ingest/ACL 库、由 `bytebeamio/uplink` 产品化而来的 Excalibur `device-agent`、TimescaleDB 遥测存储，以及 Bun + TypeScript + Next.js + Tailwind Console。

本项目不是 Bytebeam 协议兼容实现。设备 topic、设备认证 JSON、命令 payload、控制面 API 都使用 Excalibur 自有 `v1/p/{project_id}/d/{device_id}/...` 协议模型。服务端不会为 Bytebeam 的 `/tenants/...` topic 或 API 提供兼容层。

## 当前实现状态

| 领域 | 当前状态 |
| --- | --- |
| 控制面 API | `backend/apps/api` 可运行，支持 `STORAGE_BACKEND=memory` 和 `STORAGE_BACKEND=timescale`，提供 OpenAPI JSON。 |
| 身份认证 | Argon2id 密码哈希、SQL-backed Bearer session、refresh token rotation/reuse detection 和 API key hash 存储已实现；HttpOnly cookie、邮箱验证和邀请仍待生产化。 |
| 多租户 | `org -> project -> device` 模型已实现，API 和 store 显式按 org/project 校验作用域。 |
| MQTT 协议 | `backend/crates/device-protocol` 定义 topic、payload、auth JSON 和校验逻辑。 |
| MQTT ingest | `backend/apps/mqtt-ingest` 可启动本地 `rumqttd` broker，订阅 Excalibur topic 并写入 Store；生产 mTLS 连接身份 hook 仍待硬化。 |
| Device Agent | `device-agent/` 已从 uplink 产品化，改为 Excalibur 原生 topic、JSON command payload、OTA payload 和 CSR/dev auth JSON。 |
| 存储 | TimescaleDB migration 和 SQL repository 均已接入；`timescale` 模式持久化控制面、telemetry、session/refresh token 和 API key，`memory` 模式仍是进程内开发实现。 |
| 前端 | `frontend/` 是 Next.js App Router Console，已接入当前 REST API，支持注册/登录、默认 workspace bootstrap、设备 provisioning、telemetry ingest、actions、alerts、audit 和一键本地闭环 demo。 |
| 部署 | `docker-compose.yml` 和 `infra/helm/excalibur` 提供 TimescaleDB、NATS、RustFS、API、MQTT ingest、worker、frontend scaffold。 |

## 文档读法

- 从 [产品目标与边界](overview.md) 理解项目要复刻的能力边界。
- 从 [架构总览](architecture.md) 理解控制面、MQTT ingest、worker、TimescaleDB、NATS、RustFS、device-agent、Console 的关系。
- 从 [快速开始](getting-started.md) 启动本地开发环境。
- 从 [设备协议](device-protocol.md) 和 [Device Agent 总览](device-agent/overview.md) 实现设备端接入。
- 从 [生产化路线图](roadmap.md) 区分当前 scaffold 与商业生产环境仍需补齐的部分。

## 本文档如何构建

本文档使用 mdBook 组织：

```bash
mdbook build docs
mdbook serve docs --open
```

构建产物写入 `docs/book/`，该目录是生成物，不应提交。
