# 基础设施与部署

Excalibur 当前提供 Docker Compose 和 Helm 两套部署 scaffold。

## Docker Compose

`docker-compose.yml` 包含：

| 服务 | 镜像/构建 | 说明 |
| --- | --- | --- |
| `timescaledb` | `timescale/timescaledb:latest-pg16` | 本地数据库，挂载 `backend/migrations` 初始化 schema。 |
| `nats` | `nats:2.10-alpine` | 开启 JetStream 和 monitoring。 |
| `rustfs` | `rustfs/rustfs:latest` | S3-compatible object storage。 |
| `api` | `backend/Dockerfile` | `excalibur-api`，默认 `STORAGE_BACKEND=timescale`。 |
| `mqtt-ingest` | `backend/Dockerfile` | `excalibur-mqtt-ingest` rumqttd broker + ingest runtime。 |
| `worker` | `backend/Dockerfile` | `excalibur-worker` background process。 |
| `frontend` | `frontend/Dockerfile` | Next.js Console。 |

启动基础设施：

```bash
docker compose up timescaledb nats rustfs
```

RustFS 本地配置：

| 配置 | 值 |
| --- | --- |
| S3 API | `http://localhost:9000` |
| Console | `http://localhost:9001` |
| Compose service endpoint | `http://rustfs:9000` |
| Access key | `excalibur` |
| Secret key | `excalibur-secret` |
| Data volume | `rustfs-data:/data` |

应用容器通过 `S3_ENDPOINT=http://rustfs:9000` 访问 RustFS。开发机上的 SDK 或脚本应使用 `http://localhost:9000`。

启动全栈：

```bash
docker compose up --build
```

本地端口：

| 服务 | 端口 |
| --- | --- |
| API | `8080` |
| MQTT | `1883` |
| Frontend | `3000` |
| RustFS Console | `9001` |

## 环境变量

| 变量 | 服务 | 说明 |
| --- | --- | --- |
| `API_ADDR` | api | Axum bind address，默认 `0.0.0.0:8080`。 |
| `STORAGE_BACKEND` | api/mqtt-ingest | `memory` 或 `timescale`。 |
| `DATABASE_URL` | api/mqtt-ingest/worker | TimescaleDB DSN。 |
| `NATS_URL` | api/mqtt-ingest/worker | NATS DSN。 |
| `S3_ENDPOINT` | api/worker | S3-compatible endpoint，默认指向 RustFS `http://rustfs:9000`。 |
| `MQTT_LISTEN` | mqtt-ingest | rumqttd MQTT v4 bind address，默认 `0.0.0.0:1883`。 |
| `DEVICE_MQTT_BROKER` | api | provisioning auth JSON 返回给设备的 broker host。 |
| `DEVICE_MQTT_PORT` | api | provisioning auth JSON 返回给设备的 broker port。 |
| `NEXT_PUBLIC_API_BASE_URL` | frontend | Console 调用 API 的 base URL。 |

生产必须把数据库密码、RustFS/S3 凭证、CA key、JWT/session secrets 放入 secret manager，而不是明文 values。

## Helm chart

Chart 路径：

```text
infra/helm/excalibur
```

当前包含：

- `api-deployment.yaml`
- `backend-workers.yaml`
- `frontend-deployment.yaml`
- `migration-job.yaml`
- `migration-configmap.yaml`
- `values.yaml`

校验：

```bash
helm lint infra/helm/excalibur
```

默认 values：

- API replicas: 1。
- MQTT ingest replicas: 2，Service port: 1883。
- Worker replicas: 1。
- Frontend replicas: 2。
- Migrations enabled。
- `STORAGE_BACKEND=timescale` 默认启用持久化 SQL repository；快速临时开发可改为 `memory`。

## Migration job

Helm chart 的 migration job 使用 `postgres:16-alpine` 执行版本化 SQL。Job 会：

- 创建 `schema_migrations`。
- 对已有 001 初始库做 baseline，避免 upgrade 时重跑 `001_initial.sql`。
- 按文件名顺序执行 `/migrations/*.sql`。
- 对单个 migration 使用 `ON_ERROR_STOP` 和 `--single-transaction`。

生产环境仍需要：

- 失败回滚/暂停策略。
- 与应用 rollout 顺序绑定。
- 对 Timescale policy 变更做单独验证。

## Kubernetes 生产建议

生产 chart 应增加：

- Ingress 和 TLS。
- MQTT listener Service，区分 8883 TLS。
- PodDisruptionBudget。
- HPA 或 KEDA。
- NetworkPolicy。
- Secret/ExternalSecret。
- ServiceMonitor/PodMonitor。
- Resource request/limit 按压测结果调优。
- API、worker、mqtt-ingest 分离 service account。
- Backup CronJob。
- Stateful dependencies 使用托管服务或独立 chart，不建议把生产数据库或 RustFS 当作应用 chart 子资源。
