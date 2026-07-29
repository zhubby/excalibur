# Telemetry 与 Shadow

`device-agent` 把业务数据、系统指标、日志和 metrics 分为 streams。普通 stream 发布 telemetry JSON array，shadow stream 发布最新 object。

## Telemetry 批量格式

普通 stream 会被序列化为 JSON array：

```json
[
  {
    "sequence": 1,
    "timestamp": 1785313800000,
    "speed": 12.4,
    "rpm": 1800
  },
  {
    "sequence": 2,
    "timestamp": 1785313805000,
    "speed": 12.9,
    "rpm": 1840
  }
]
```

topic：

```text
v1/p/{project_id}/d/{device_id}/telemetry/{stream}
```

服务端解析后写入 `TelemetryPoint`：

- `project_id`
- `device_id`
- `stream`
- `sequence`
- `ts`
- `payload`
- `ingested_at`

## 动态 stream

如果 TOML 中没有显式配置 topic，agent 会自动生成：

```text
v1/p/{project_id}/d/{device_id}/telemetry/{stream_name}
```

这允许应用侧快速增加业务 stream。但生产平台仍应通过 stream definition 管理字段 schema、retention、dashboard 默认配置和 alert rule 校验。

## 本地持久化

Agent 的 serializer 在网络慢、event loop 阻塞或 MQTT client queue 满时，会把 publish packet 写入本地 storage：

- `persistence.max_file_size`
- `persistence.max_file_count`
- `persistence_path`

恢复时 agent 会检查 packet topic 是否以当前设备的 topic prefix 开头：

```text
v1/p/{project_id}/d/{device_id}
```

这可以避免换设备身份后误发旧 backlog。

## Shadow

`device_shadow` collector 定期产生 shadow payload。serializer 对 shadow topic 特殊处理：

- 不发布 JSON array。
- 从 buffer 中选择最新一个 object。
- 发布到 `v1/p/{project_id}/d/{device_id}/shadow`。

示例：

```json
{
  "firmware": {
    "motor": "3.2.1"
  },
  "network": {
    "rssi": -58
  },
  "uptime_seconds": 86400
}
```

服务端更新 device：

- `latest_shadow`
- `last_seen_at`

## 日志和 system stats

Linux 日志 collector 基于 journalctl 配置；system stats collector 默认输出多条 `device_agent_*` stream。它们都属于 telemetry stream，不属于 shadow。

建议 stream 命名：

| 用途 | Stream |
| --- | --- |
| 系统健康 | `device_agent_system_stats` |
| 网络 | `device_agent_network_stats` |
| 进程 | `device_agent_process_stats` |
| 磁盘 | `device_agent_disk_stats` |
| 业务电池 | `battery` |
| 业务电机 | `motor` |
| 日志 | `logs` |

## 服务端查询

当前 API：

```text
GET /api/v1/telemetry?project_id=...&device_id=...&stream=...&limit=100
```

生产 Dashboard query 应增加：

- 时间范围。
- downsample interval。
- aggregation functions。
- pagination cursor。
- continuous aggregate。
- CSV/Parquet export。
