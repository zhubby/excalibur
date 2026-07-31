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
| `CORS_ALLOWED_ORIGINS` | api | 允许携带 cookie 调用 API 的 Console origins，逗号分隔；默认只包含本地开发端口。 |
| `SESSION_COOKIE_SECURE` | api | 设为 `true`/`1` 时 auth cookies 带 `Secure`，生产 TLS 环境必须开启。 |
| `NATS_URL` | api/mqtt-ingest/worker | NATS DSN。 |
| `S3_ENDPOINT` | api/mqtt-ingest | S3-compatible internal endpoint，默认指向 RustFS `http://rustfs:9000`。 |
| `S3_PUBLIC_ENDPOINT` | api/mqtt-ingest | 签发给 Console/agent 的 public S3 endpoint。 |
| `S3_BUCKET` | api/mqtt-ingest | firmware/diagnostics/export object bucket。 |
| `S3_REGION` | api/mqtt-ingest | S3 SigV4 region。 |
| `S3_ACCESS_KEY_ID` / `S3_SECRET_ACCESS_KEY` | api/mqtt-ingest | S3 SigV4 signing credentials，生产必须来自 secret manager。 |
| `API_AUTH_RATE_LIMIT_MAX_ATTEMPTS` | api | register/login 在窗口内允许的最大尝试次数。 |
| `API_AUTH_RATE_LIMIT_WINDOW_SECONDS` | api | auth rate limit 窗口秒数。 |
| `EXCALIBUR_CA_PRIVATE_KEY_PEM` | api | 设备证书签发 CA private key PEM；生产必须通过 Helm Secret/secret manager 注入。 |
| `EXCALIBUR_ALLOW_DEV_CA` | api | 允许 API 使用内置 dev CA；仅本地开发设为 `true`。 |
| `EXCALIBUR_ENABLE_DEV_AUTH` | api | 启用返回 inline private key 的 dev-auth provisioning；仅本地开发/批量实验设为 `true`。 |
| `MQTT_LISTEN` | mqtt-ingest | rumqttd MQTT v4 bind address，默认 `0.0.0.0:1883`。 |
| `MQTT_TELEMETRY_BUFFER` | mqtt-ingest | `auto`/`direct`/`nats`，有 NATS 时默认进入 JetStream buffer。 |
| `MQTT_TELEMETRY_NATS_SUBJECT` | mqtt-ingest | telemetry envelope subject。 |
| `MQTT_TELEMETRY_NATS_STREAM` | mqtt-ingest | telemetry JetStream stream name。 |
| `MQTT_COMMAND_BRIDGE` | mqtt-ingest | `auto`/`disabled`/`nats`，把 worker command bus 转发到 MQTT broker。 |
| `MQTT_COMMAND_NATS_SUBJECT` | mqtt-ingest | command bridge 订阅 subject。 |
| `MQTT_COMMAND_NATS_STREAM` | mqtt-ingest | command bridge durable JetStream stream。 |
| `MQTT_COMMAND_DELIVERY_SUBJECT` | mqtt-ingest | command bridge push consumer delivery subject。 |
| `MQTT_COMMAND_DURABLE` | mqtt-ingest | command bridge durable consumer name。 |
| `MQTT_COMMAND_QUEUE_GROUP` | mqtt-ingest | command bridge queue group。 |
| `MQTT_COMMAND_DEAD_LETTER_SUBJECT` | mqtt-ingest | invalid command envelope dead-letter subject。 |
| `MQTT_COMMAND_DOWNLOAD_URL_TTL_SECONDS` | mqtt-ingest | command bridge 发布 OTA MQTT command 前即时签发 download URL 的 TTL。 |
| `MQTT_REQUIRE_CERT_FINGERPRINT_USERNAME` | mqtt-ingest | 设为 `true` 时，用 certificate fingerprint 建立连接身份，并按连接身份校验 publish/subscribe topic。 |
| `WORKER_TELEMETRY_NATS_SUBJECT` | worker | telemetry ingest source subject。 |
| `WORKER_TELEMETRY_NATS_STREAM` | worker | telemetry durable consumer 所属 stream。 |
| `WORKER_TELEMETRY_DELIVERY_SUBJECT` | worker | JetStream push consumer delivery subject。 |
| `WORKER_TELEMETRY_DEAD_LETTER_SUBJECT` | worker | invalid telemetry envelope dead-letter subject。 |
| `WORKER_ACTION_COMMAND_SUBJECT` | worker | action dispatcher 发布 command envelope 的 subject。 |
| `WORKER_ACTION_COMMAND_STREAM` | worker | action command JetStream stream name。 |
| `WORKER_ACTION_COMMAND_DEAD_LETTER_SUBJECT` | worker | command dispatch dead-letter subject。 |
| `WORKER_ACTION_TIMEOUT_SECONDS` | worker | running action target 超时时间。 |
| `WORKER_ALERT_SCAN_INTERVAL_MS` | worker | alert rule 扫描间隔。 |
| `WORKER_ALERT_DEFAULT_OFFLINE_AFTER_SECONDS` | worker | offline alert 未单独配置时的默认离线阈值。 |
| `WORKER_ALERT_DEFAULT_WINDOW_SECONDS` | worker | threshold/window alert 默认查询窗口。 |
| `WORKER_ALERT_NOTIFICATION_SUBJECT` | worker | alert notification stub 发布 subject。 |
| `DEVICE_MQTT_BROKER` | api | provisioning auth JSON 返回给设备的 broker host。 |
| `DEVICE_MQTT_PORT` | api | provisioning auth JSON 返回给设备的 broker port。 |
| `NEXT_PUBLIC_API_BASE_URL` | frontend | Console 调用 API 的 base URL。 |

生产必须把数据库密码、RustFS/S3 凭证、CA key、JWT/session secrets 放入 secret manager，而不是明文 values。`docker-compose.yml` 为本地开发显式启用 `EXCALIBUR_ALLOW_DEV_CA=true` 和 `EXCALIBUR_ENABLE_DEV_AUTH=true`；Helm 默认关闭这两个开关，未配置真实 CA Secret 时 API 会 fail closed。

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
- MQTT ingest replicas: 1，Service port: 1883。当前本地 rumqttd broker + command bridge 只能安全默认单副本；多副本需要 sticky/device ownership routing 或外部共享 broker 后再开启。
- Worker replicas: 1。
- Frontend replicas: 2。
- Migrations enabled。
- `STORAGE_BACKEND=timescale` 默认启用持久化 SQL repository；快速临时开发可改为 `memory`。

Command bus rolling upgrade 注意：worker 和 mqtt-ingest 会在启动时校验 JetStream command stream subjects、consumer delivery subject、deliver group 和 explicit ack policy。已有环境如果手工创建过不兼容的 `EXCALIBUR_COMMANDS` stream 或 `excalibur-mqtt-command-bridge` durable consumer，启动会 fail closed；升级前应通过 JetStream API/CLI update 对应 stream subjects 和 consumer config，或在确认无待处理 command 后删除旧 consumer 让应用重建。

生产部署需先创建 CA private key Secret，并在 values 中引用：

```yaml
api:
  caPrivateKeySecret:
    name: excalibur-ca
    key: ca-private-key.pem
env:
  EXCALIBUR_ALLOW_DEV_CA: "false"
  EXCALIBUR_ENABLE_DEV_AUTH: "false"
```

## Migration job

Helm chart 的 migration job 使用 `postgres:16-alpine` 执行版本化 SQL。Job 会：

- 创建 `schema_migrations`。
- 创建 `schema_migration_events`，记录 `applying`、`applied`、`failed`。
- 对已有完整 001 初始库做 race-safe baseline；如果检测到只存在部分旧表，会失败并提示缺失对象，避免 upgrade 时误跳过 `001_initial.sql`。
- 按文件名顺序执行 `/migrations/*.sql`，并用 `pg_advisory_lock(hashtext('excalibur_schema_migrations'))` 串行化。
- 对单个 migration 使用 `ON_ERROR_STOP` 和显式 `BEGIN`/`COMMIT`；失败会写 `failed` 事件并让 Helm retry/backoff。
- 002 upgrade migration 会在加唯一索引和复合外键前输出冲突数据样例，方便先修旧数据再重试。
- 对已有大量 telemetry 的生产库，002 的 telemetry index 调整和 dedupe 回填应按维护窗口或拆分 online backfill 执行。
- 通过 `migrations.activeDeadlineSeconds` 给 hook 设置最大运行时长，避免锁等待或长 migration 无限阻塞 release。

生产环境仍需要：

- 与应用 rollout 顺序绑定。
- 对 Timescale policy 变更做单独验证。
- 定期演练 migration 失败恢复流程。

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
