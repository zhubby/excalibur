# Agent 配置

`device-agent` 使用两个配置文件：

- auth JSON：设备身份、可选 broker fallback、证书。
- TOML config：stream、MQTT client、上游发现、collector、downloader、console、remote shell 等运行配置。

## Auth JSON 字段

| 字段 | 必需 | 说明 |
| --- | --- | --- |
| `broker` | discovery 关闭时必需 | MQTT broker host。启用上游发现时可省略，或作为 discovery 失败后的 fallback。 |
| `port` | discovery 关闭时必需 | MQTT port。启用上游发现时可省略，或作为 discovery 失败后的 fallback。 |
| `project_id` | 是 | Excalibur project ID。 |
| `device_id` | 是 | Excalibur device ID。 |
| `authentication.ca_certificate` | 建议 | Broker CA PEM。 |
| `authentication.device_certificate` | 建议 | Device certificate PEM。 |
| `authentication.device_private_key` | dev only | Inline private key。 |
| `authentication.device_private_key_path` | 生产推荐 | 本地 private key path。 |

如果同时存在 inline key 和 key path，agent 当前优先使用 inline key，然后回退到 path。

默认启用 Tailscale 上游发现时，auth JSON 可以只包含 `project_id`、`device_id` 和证书字段。若 `[upstream_discovery] enabled = false`，`broker` 和 `port` 会恢复为必填字段，agent 使用静态地址，不访问 Tailscale LocalAPI。

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

## Tailscale 上游发现

默认配置：

```toml
[upstream_discovery]
enabled = true
server_tag = "tag:excalibur-server"
api_ready_port = 8080
mqtt_plaintext_port = 1883
mqtt_tls_port = 8883
probe_timeout_ms = 2000
# socket_path = "/var/run/tailscale/tailscaled.sock"
```

启用后，agent 启动时会通过 `tailscale-localapi` 读取本机 `tailscaled` status，要求 Tailscale backend state 为 `Running`，并在同一 tailnet 中查找在线、非本机、带 `tag:excalibur-server` 的 peer。生产 tailnet 必须把 Excalibur server 节点标记为该 tag；不使用 hostname 匹配或端口扫描。

候选 peer 必须同时通过：

- `GET http://<tailscale-ip>:8080/ready`
- MQTT TCP connect

agent 优先使用 Tailscale IPv4 地址作为内存中的 broker host。存在 mTLS auth 时默认探测 MQTT `8883`，否则探测 `1883`。mTLS 部署需要确保 broker server certificate 覆盖该 Tailscale IPv4 SAN，或在部署设计中明确提供匹配的 TLS server name；否则 discovery 可通过 TCP 探测但实际 TLS 连接会失败。如果发现失败但 auth JSON 同时包含 `broker` 和 `port`，agent 会记录 fallback 并按旧静态配置连接；如果没有 fallback，则启动失败并输出明确错误。若多个 tagged peer 同时通过探测，v1 会启动失败，避免非确定性路由，即使 auth JSON 提供了静态 fallback 也不会吞掉该歧义。

`socket_path` 可指定本机 tailscaled Unix socket。未配置时依次尝试 `/var/run/tailscale/tailscaled.sock` 和 `/run/tailscale/tailscaled.sock`。

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
