# Device Agent 总览

`device-agent/` 是 Excalibur 官方 Linux-first 设备运行时。它来自 `bytebeamio/uplink`，但已经产品化为 Excalibur 自有 agent，并改写为 Excalibur 原生 MQTT 协议。

## 核心职责

- 读取 auth JSON，使用 mTLS 连接 broker。
- 从本机应用、系统 collector、日志 collector、stdin、模拟器等来源采集数据。
- 按 stream 批量序列化 telemetry JSON array。
- 对网络故障和慢 event loop 做本地缓冲和恢复。
- 发布 latest device shadow。
- 订阅 command topic 并执行 Actions。
- 对 OTA 下载做断点恢复、checksum 校验和安装交接。
- 发布 command status。
- 采集 logs/system stats 作为 telemetry streams。
- 保留 remote shell beta 能力，但默认关闭。

## 工作区边界

`device-agent` 保持独立 Rust workspace：

```text
device-agent/
  Cargo.toml
  device-agent/
    Cargo.toml
    src/
  tools/
  scripts/
  docs/
```

原因：

- 上游衍生依赖和设备端工具链不影响后端构建。
- Android 残留代码可以保留，但 Linux CI 是首阶段验收。
- 设备端发布节奏可以和 SaaS 后端分离。

## 启动方式

二进制名为 `device_agent`。启动参数：

```bash
device_agent -a /etc/excalibur/auth.json -c /etc/excalibur/device-agent.toml -v
```

参数：

| 参数 | 说明 |
| --- | --- |
| `-a` | auth JSON 路径，必需。 |
| `-c` | agent config TOML 路径，可选。 |
| `-v` | 日志级别，重复增加详细程度。 |
| `-m` | 指定模块日志过滤。 |
| `--sha` | 输出构建 Git SHA 前 8 位。 |

## 与平台交互

```text
device-agent
  publish telemetry JSON array -> v1/p/{project_id}/d/{device_id}/telemetry/{stream}
  publish latest shadow        -> v1/p/{project_id}/d/{device_id}/shadow
  subscribe commands           <- v1/p/{project_id}/d/{device_id}/commands
  publish command status array -> v1/p/{project_id}/d/{device_id}/commands/status
```

## 默认内置 streams

| Stream | Topic | 默认用途 |
| --- | --- | --- |
| `action_status` | `.../commands/status` | action progress 和终态。 |
| `device_shadow` | `.../shadow` | 最新 shadow object。 |
| `logs` | `.../telemetry/logs` | Linux journalctl 或 Android logcat。 |
| `device_agent_*_stats` | `.../telemetry/device_agent_*` | system stats collector。 |
| `device_agent_mqtt_metrics` | `.../telemetry/device_agent_mqtt_metrics` | MQTT client metrics。 |

## 当前限制

- 生产 CA、证书吊销下发、broker connect hook 仍待后端接入。
- Remote shell 默认关闭，不能在未审计、未授权情况下开放。
- 上游文档和部分工具脚本仍可能保留历史命名，根级文档以 Excalibur 协议为准。
