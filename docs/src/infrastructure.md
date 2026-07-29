# 基础设施与部署

Excalibur 当前提供 Docker Compose 和 Helm 两套部署 scaffold。

## Docker Compose

`docker-compose.yml` 包含：

| 服务 | 镜像/构建 | 说明 |
| --- | --- | --- |
| `timescaledb` | `timescale/timescaledb:latest-pg16` | 本地数据库，挂载 `backend/migrations` 初始化 schema。 |
| `nats` | `nats:2.10-alpine` | 开启 JetStream 和 monitoring。 |
| `minio` | `minio/minio:latest` | S3-compatible object storage。 |
| `api` | `backend/Dockerfile` | `excalibur-api`，当前 `STORAGE_BACKEND=memory`。 |
| `mqtt-ingest` | `backend/Dockerfile` | `excalibur-mqtt-ingest` process boundary。 |
| `worker` | `backend/Dockerfile` | `excalibur-worker` background process。 |
| `frontend` | `frontend/Dockerfile` | Next.js Console。 |

启动基础设施：

```bash
docker compose up timescaledb nats minio
```

启动全栈：

```bash
docker compose up --build
```

## 环境变量

| 变量 | 服务 | 说明 |
| --- | --- | --- |
| `API_ADDR` | api | Axum bind address，默认 `0.0.0.0:8080`。 |
| `STORAGE_BACKEND` | api | 当前只支持 `memory`。 |
| `DATABASE_URL` | api/mqtt-ingest/worker | TimescaleDB DSN。 |
| `NATS_URL` | api/mqtt-ingest/worker | NATS DSN。 |
| `S3_ENDPOINT` | api/worker | S3-compatible endpoint。 |
| `NEXT_PUBLIC_API_BASE_URL` | frontend | Console 调用 API 的 base URL。 |

生产必须把数据库密码、S3 凭证、CA key、JWT/session secrets 放入 secret manager，而不是明文 values。

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
- MQTT ingest replicas: 2。
- Worker replicas: 1。
- Frontend replicas: 2。
- Migrations enabled。
- `STORAGE_BACKEND=memory`，生产前必须改为 SQL repository 支持后的 `timescale` 或等效值。

## Migration job

Helm chart 的 migration job 使用 `postgres:16-alpine` 执行初始 SQL。生产环境需要：

- 迁移版本表。
- 幂等 migration runner。
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
- Stateful dependencies 使用托管服务或独立 chart，不建议把生产数据库当作应用 chart 子资源。
