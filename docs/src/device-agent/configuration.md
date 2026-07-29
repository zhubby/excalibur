# Agent 配置

`device-agent` 使用两个配置文件：

- auth JSON：设备身份、broker、证书。
- TOML config：stream、MQTT client、collector、downloader、console、remote shell 等运行配置。

## Auth JSON 字段

| 字段 | 必需 | 说明 |
| --- | --- | --- |
| `broker` | 是 | MQTT broker host。 |
| `port` | 是 | MQTT TLS 端口，通常是 `8883`。 |
| `project_id` | 是 | Excalibur project ID。 |
| `device_id` | 是 | Excalibur device ID。 |
| `authentication.ca_certificate` | 建议 | Broker CA PEM。 |
| `authentication.device_certificate` | 建议 | Device certificate PEM。 |
| `authentication.device_private_key` | dev only | Inline private key。 |
| `authentication.device_private_key_path` | 生产推荐 | 本地 private key path。 |

如果同时存在 inline key 和 key path，agent 当前优先使用 inline key，然后回退到 path。

## 默认配置重点

默认配置定义在 `device-agent/device-agent/src/lib.rs` 的 `DEFAULT_CONFIG`：

```toml
enable_remote_shell = false
enable_stdin_collector = false
prioritize_live_data = false
wait_for_disk = true
actions_subscription = "v1/p/{project_id}/d/{device_id}/commands"
max_stream_count = 20
```

Topic 中的 `{project_id}` 和 `{device_id}` 会在启动时替换为 auth JSON 中的实际值。

## MQTT 配置

```toml
[mqtt]
max_packet_size = 256000
max_inflight = 100
keep_alive = 30
network_timeout = 30
```

建议：

- `max_packet_size` 与服务端 broker limit 对齐。
- `max_inflight` 根据网络质量和 broker backpressure 调整。
- `keep_alive` 保持在能及时发现断线但不过度 ping 的范围。

## Streams

每个 stream 可以配置：

```toml
[streams.motor]
topic = "v1/p/{project_id}/d/{device_id}/telemetry/motor"
batch_size = 50
flush_period = 10
compression = "Lz4"
persistence = { max_file_size = 1048576, max_file_count = 3 }
priority = 50
```

字段含义：

| 字段 | 说明 |
| --- | --- |
| `topic` | 为空时自动生成 `.../telemetry/{stream_name}`。 |
| `batch_size` | 一个 MQTT publish 中最多包含的数据点数量。 |
| `flush_period` | 第一条数据进入 buffer 后的最大等待秒数。 |
| `compression` | `Disabled` 或 `Lz4`。 |
| `persistence` | 网络异常时本地持久化 backlog。 |
| `priority` | 越高越优先发送，action status 默认 255。 |

## Built-in stream 特殊行为

`action_status`：

- topic 是 `.../commands/status`。
- payload 是 status JSON array。
- 默认持久化，避免设备重启丢失关键状态。

`device_shadow`：

- topic 是 `.../shadow`。
- serializer 只发布 buffer 中最后一个 object。
- 不按 telemetry JSON array 发布。

## System stats

默认开启：

```toml
[system_stats]
enabled = true
process_names = ["device_agent"]
update_period = 10
stream_size = 16
```

启动时会创建：

- `device_agent_disk_stats`
- `device_agent_network_stats`
- `device_agent_processor_stats`
- `device_agent_process_stats`
- `device_agent_component_stats`
- `device_agent_system_stats`

这些都走 telemetry topic。

## Downloader 与 OTA

```toml
[downloader]
actions = [
  { name = "ota.install" },
  { name = "send_file" },
  { name = "send_script" }
]
path = "/var/tmp/ota-file"
```

生产建议把下载路径放在专用分区，限制容量，并由 installer 做 hash/signature 验证后再切换版本。

## Remote shell

```toml
enable_remote_shell = false
```

该能力必须默认关闭。启用条件应至少包括：

- project beta flag。
- 用户 RBAC 权限。
- 短时授权 token。
- 全量 audit。
- 可配置命令限制和 session timeout。
