# 快速开始

本页用于在本地跑起 Excalibur scaffold，并验证后端、前端和 device-agent 的基本检查。

## 前置依赖

- Rust stable toolchain。
- Bun。
- Docker 或兼容 Docker Compose 的运行时。
- 可选：`mdbook`，用于构建本文档。
- 可选：Helm，用于校验 Kubernetes chart。

设备端 workspace 包含 `rust-toolchain.toml` 和上游衍生目标配置。为了避免本地自动同步额外 target，建议在设备端命令中显式使用 stable：

```bash
cd device-agent
RUSTUP_TOOLCHAIN=stable cargo test --workspace
```

## 启动基础设施

```bash
docker compose up timescaledb nats minio
```

服务端口：

| 服务 | 端口 | 用途 |
| --- | --- | --- |
| TimescaleDB | `5432` | PostgreSQL 控制面表和 telemetry hypertable。 |
| NATS | `4222` | JetStream 数据/命令缓冲。 |
| NATS monitoring | `8222` | NATS 监控。 |
| MinIO API | `9000` | S3-compatible object storage。 |
| MinIO Console | `9001` | 本地对象存储控制台。 |

数据库初始化会挂载 `backend/migrations` 到 TimescaleDB 容器的 init 目录。

## 后端

运行测试：

```bash
cd backend
RUSTUP_TOOLCHAIN=stable cargo test --workspace
RUSTUP_TOOLCHAIN=stable cargo test --workspace --all-features
```

启动 API：

```bash
cd backend
STORAGE_BACKEND=memory RUSTUP_TOOLCHAIN=stable cargo run -p excalibur-api
```

健康检查：

```bash
curl http://localhost:8080/health
```

OpenAPI：

```bash
curl http://localhost:8080/api/v1/openapi.json
```

注意：当前只有 `STORAGE_BACKEND=memory` 可运行。设置 `postgres` 或 `timescale` 会 fail fast，因为 SQL repositories 尚未实现。

## 前端 Console

```bash
cd frontend
bun install
bun run dev
```

默认地址是 `http://localhost:3000`。当前 Console 使用静态 demo 数据展示 fleet、telemetry、actions、alerts 和 device-agent provisioning 面板；后续会接入 OpenAPI 生成的 TS client。

验证：

```bash
cd frontend
bun run typecheck
bun run test
bun run build
```

## Device Agent

运行测试：

```bash
cd device-agent
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo test --workspace
```

最小运行形态需要 auth JSON：

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
  }
}
```

生产路径应由设备本地生成私钥和 CSR，再调用 API 签发证书。开发和批量实验可以使用 dev auth JSON，但该路径会返回 inline private key，不应进入生产 fleet。

## 本地 Compose 全栈

`docker-compose.yml` 包含 `api`、`mqtt-ingest`、`worker` 和 `frontend` 服务。当前 API 容器仍使用 `STORAGE_BACKEND=memory`，因此 Compose 更适合验证镜像构建和进程拓扑，不代表持久化生产部署。

```bash
docker compose up --build
```

## 文档

```bash
mdbook build docs
mdbook serve docs --open
```
