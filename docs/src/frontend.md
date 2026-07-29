# 前端 Console

`frontend/` 是 Excalibur 操作员 Console，使用 Bun、TypeScript、Next.js App Router 和 Tailwind。

## 技术栈

| 依赖 | 用途 |
| --- | --- |
| `next` | App Router 和 SSR/静态页面构建。 |
| `react` | UI。 |
| `tailwindcss` | 样式系统。 |
| `lucide-react` | 图标。 |
| `@tanstack/react-table` | 表格能力。 |
| `uplot` | 高性能时序图目标库。 |
| `vitest` | 单元测试。 |

## 当前页面

首页位于 `frontend/src/app/page.tsx`，当前是 Console scaffold，包含：

- Sidebar navigation。
- Project header。
- Metric strip。
- Telemetry panel。
- Device table。
- Device-agent provisioning/status panel。
- Action queue panel。
- Alert panel。
- Protocol topic 展示。

Demo 数据来自 `frontend/src/lib/data.ts`。

## 组件边界

| 组件 | 职责 |
| --- | --- |
| `Sidebar` | Fleet、Streams、Actions、Firmware、Security 导航入口。 |
| `ProjectHeader` | 当前 org/project/region 的上下文展示。 |
| `MetricStrip` | connected devices、telemetry ingest、open actions、alert pressure。 |
| `TelemetryPanel` | stream health 和 telemetry trend。 |
| `DeviceTable` | 设备列表、状态、firmware、shadow、last seen。 |
| `DeviceAgentPanel` | CSR/dev auth 下载入口、agent status、OTA/diagnostics/shell controls。 |
| `ActionQueuePanel` | action 进度摘要。 |
| `AlertPanel` | alert rule 状态摘要。 |

## Protocol helper

`frontend/src/lib/protocol.ts` 提供 topic helper：

- `telemetryTopic(projectId, deviceId, stream)`
- `shadowTopic(projectId, deviceId)`
- `commandTopic(projectId, deviceId)`
- `commandStatusTopic(projectId, deviceId)`
- `parseTopicKind(topic)`
- `clampProgress(value)`

这些 helper 当前有 Vitest 覆盖。后续应从后端 device protocol 或 OpenAPI 生成共享 contract，避免 Rust/TS 漂移。

## API client 目标

生产前端应：

1. 从 `/api/v1/openapi.json` 生成 TS client。
2. 用 HttpOnly cookie session，避免在 JS 中持有长期 token。
3. 对 project switch 做全局 context。
4. 所有请求显式带 `project_id` 或 `org_id`。
5. 对 RBAC 禁用按钮和路由，而不仅是隐藏入口。
6. 对 SSE 建立单 project connection。
7. Remote shell 使用单独 WebSocket tunnel，并且只在 beta flag 和权限满足时显示。

## UI 设计原则

Console 是运营工具，不是营销页。应优先：

- 信息密度适中，便于 scanning。
- 设备、stream、action、alert 可快速过滤和比较。
- 危险操作明确二次确认。
- OTA、cert revoke、remote shell 等操作必须显示 scope 和目标数量。
- Dashboard panel 避免过度装饰，突出数据可读性和延迟/范围。

## 本地命令

```bash
cd frontend
bun install
bun run dev
bun run typecheck
bun run test
bun run build
```
