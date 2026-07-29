# 仓库结构

```text
backend/
  apps/
    api/                 Axum REST API、SSE、OpenAPI。
    mqtt-ingest/         MQTT ACL、topic parser 调用、payload ingest 边界。
    worker/              后台任务进程 scaffold。
  crates/
    device-protocol/     MQTT topic、device auth JSON、command payload 类型。
    domain/              用户、组织、项目、设备、证书、stream、action 等领域模型。
    storage/             Store trait 边界和 in-memory development store。
  migrations/            TimescaleDB schema。

device-agent/
  device-agent/          Excalibur Linux-first device-agent Rust crate。
  docs/                  上游衍生设备端旧文档和配置示例。
  scripts/               设备端启动、systemd、OTA 示例脚本。
  tools/                 模拟器、system-stats、工具脚本。

frontend/
  src/app/               Next.js App Router 页面和全局样式。
  src/components/        Console UI 组件。
  src/lib/               静态 demo 数据和协议 helper。

infra/
  helm/excalibur/        Kubernetes Helm chart。

examples/
  device-simulator/      Rust 设备模拟器 skeleton。

docs/
  book.toml              mdBook 配置。
  src/                   本文档源文件。

docker-compose.yml       本地 TimescaleDB、NATS、MinIO、应用服务拓扑。
README.md                项目入口说明。
```

## Workspace 边界

后端和设备端是两个独立 Rust workspace：

- `backend/Cargo.toml` 管理 API、MQTT ingest、worker 和共享 crates。
- `device-agent/Cargo.toml` 管理从 uplink 产品化而来的设备端代码。

这个边界是故意的。设备端保留了更多上游衍生代码、工具、Android 残留和特定依赖；把它并入后端 workspace 会让后端 CI、依赖图和工具链受设备端影响。

## 文档边界

根级 `docs/` 是整个平台文档，使用 mdBook 组织。`device-agent/docs/` 是设备端 workspace 内部文档和示例，后续可以逐步清理为 Excalibur 命名，但不替代根级平台文档。

## 生成物

以下目录或文件是生成物，不应提交：

- `docs/book/`
- Rust `target/`
- Frontend `.next/`、`node_modules/`、`dist/`
- Device-agent `.persistence/`、`.downloads/`、diagnostics 缓存
- 私钥、证书、CSR、auth JSON
