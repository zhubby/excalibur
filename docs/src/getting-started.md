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
docker compose up timescaledb nats rustfs
```

服务端口：

| 服务 | 端口 | 用途 |
| --- | --- | --- |
| TimescaleDB | `5432` | PostgreSQL 控制面表和 telemetry hypertable。 |
| NATS | `4222` | JetStream 数据/命令缓冲。 |
| NATS monitoring | `8222` | NATS 监控。 |
| RustFS S3 API | `9000` | S3-compatible object storage。 |
| RustFS Console | `9001` | 本地对象存储控制台。 |
| MQTT | `1883` | 本地 rumqttd MQTT v4 listener。 |

RustFS 本地默认凭证是 `excalibur` / `excalibur-secret`。应用容器内部 endpoint 是 `http://rustfs:9000`，宿主机访问 endpoint 是 `http://localhost:9000`。

数据库初始化会挂载 `backend/migrations` 到 TimescaleDB 容器的 init 目录。
如果本地已有旧的 `timescaledb-data` volume，init SQL 不会重新执行；升级 schema 时需要手动迁移，或删除该本地开发 volume 后重建。

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

启动 SQL-backed API：

```bash
cd backend
DATABASE_URL=postgres://excalibur:excalibur@localhost:5432/excalibur \
  STORAGE_BACKEND=timescale \
  RUSTUP_TOOLCHAIN=stable \
  cargo run -p excalibur-api
```

健康检查：

```bash
curl http://localhost:8080/health
```

OpenAPI：

```bash
curl http://localhost:8080/api/v1/openapi.json
```

`memory` 适合快速开发和单元测试；`timescale` 会使用 SQL repository，并要求 `DATABASE_URL` 指向已经应用 `backend/migrations/*.sql` 的 TimescaleDB 数据库。启动时 API 会校验核心 schema、`telemetry_points` hypertable，以及 compression/retention policy 是否存在。

## MQTT broker

启动 SQL-backed MQTT broker 和 ingest runtime：

```bash
make mqtt
```

或手动运行：

```bash
cd backend
MQTT_LISTEN=0.0.0.0:1883 \
  DATABASE_URL=postgres://excalibur:excalibur@localhost:5432/excalibur \
  STORAGE_BACKEND=timescale \
  RUSTUP_TOOLCHAIN=stable \
  cargo run -p excalibur-mqtt-ingest
```

它会启动 `rumqttd`，订阅 `v1/p/+/d/+/telemetry/+`、`v1/p/+/d/+/shadow` 和 `v1/p/+/d/+/commands/status`，并把符合 Excalibur 协议的 publish 写入 store 或 JetStream。TLS listener 可通过 `MQTT_TLS_*` 启用；`MQTT_REQUIRE_CERT_FINGERPRINT_USERNAME=true` 时，agent 会把 auth JSON 中的 `certificate_fingerprint_sha256` 作为 MQTT username，runtime 会把该 fingerprint 解析成连接设备身份，并用连接身份校验 publish/subscribe topic。

## 前端 Console

```bash
cd frontend
bun install
bun run dev
```

默认地址是 `http://localhost:3000`。Console 默认调用 `http://localhost:8080`，也可以在登录页修改 API base URL。注册或登录后，空库会自动创建默认 org/project/stream/alert/dashboard；点击页面右上角 `Run loop` 会创建或复用设备、写入 shadow/telemetry、创建 diagnostics action 并回写 completed 状态，从而验证第一版控制面闭环。

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
  "broker": "localhost",
  "port": 1883,
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

`docker-compose.yml` 包含 `api`、`mqtt-ingest`、`worker` 和 `frontend` 服务。当前 API 和 MQTT ingest 容器默认使用 `STORAGE_BACKEND=timescale`，并通过 `DATABASE_URL=postgres://excalibur:excalibur@timescaledb:5432/excalibur` 连接 TimescaleDB。`mqtt-ingest` 会暴露宿主机 `1883`。若只想快速验证 API 进程，可临时改回 `memory`。

```bash
docker compose up --build
```

本机开发也可以直接使用 Makefile：

```bash
make dev-full
```

## 文档

```bash
mdbook build docs
mdbook serve docs --open
```
