import type {
  Action,
  AlertRule,
  AuditLog,
  Device,
  FirmwareArtifact,
  FirmwareRollout,
  JsonValue,
  StreamDefinition,
  TelemetryPoint,
} from "@/lib/api";
import type { ActionSummary, AlertSummary, DeviceRow, DeviceStatus, StreamSummary } from "@/lib/data";

export const SYSTEM_STREAM = "device_agent_system_stats";

export type ProjectData = {
  devices: Device[];
  streams: StreamDefinition[];
  telemetry: TelemetryPoint[];
  actions: Action[];
  firmware: FirmwareArtifact[];
  firmwareRollouts: FirmwareRollout[];
  alerts: AlertRule[];
  audit: AuditLog[];
};

export const emptyProjectData: ProjectData = {
  devices: [],
  streams: [],
  telemetry: [],
  actions: [],
  firmware: [],
  firmwareRollouts: [],
  alerts: [],
  audit: [],
};

export type DashboardStats = {
  totalDevices: number;
  onlineDevices: number;
  offlineDevices: number;
  disabledDevices: number;
  provisionedDevices: number;
  onlinePercent: number;
  telemetryRows: number;
  streamCount: number;
  openActions: number;
  totalActions: number;
  firingAlerts: number;
  totalAlerts: number;
  firmwareCount: number;
  activeFirmwareCount: number;
  rolloutCount: number;
  activeRolloutCount: number;
  deviceStatusCounts: Record<DeviceStatus, number>;
};

export function isRecord(value: JsonValue | undefined): value is Record<string, JsonValue> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function getString(record: Record<string, JsonValue> | null, key: string) {
  const value = record?.[key];
  return typeof value === "string" ? value : null;
}

export function getNumber(record: Record<string, JsonValue> | null, key: string) {
  const value = record?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function nestedRecord(record: Record<string, JsonValue> | null, key: string) {
  const value = record?.[key];
  return isRecord(value) ? value : null;
}

export function humanizeEnum(value: string) {
  return value
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/_/g, " ")
    .trim()
    .toLowerCase();
}

export function normalizeDeviceStatus(status: string): DeviceStatus {
  const value = humanizeEnum(status);
  if (value === "online" || value === "offline" || value === "disabled") {
    return value;
  }
  return "provisioned";
}

export function isTerminalAction(action: Action) {
  const state = humanizeEnum(action.state);
  return state === "completed" || state === "failed" || state === "cancelled" || state === "timed out";
}

export function isActiveRollout(rollout: FirmwareRollout) {
  const state = humanizeEnum(rollout.state);
  return state === "planned" || state === "waiting approval" || state === "running";
}

export function formatRelativeTime(iso: string | null) {
  if (!iso) {
    return "never";
  }
  const timestamp = Date.parse(iso);
  if (!Number.isFinite(timestamp)) {
    return "unknown";
  }
  const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1000));
  if (seconds < 60) {
    return `${seconds}s ago`;
  }
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ago`;
  }
  const hours = Math.round(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  return `${Math.round(hours / 24)}d ago`;
}

export function formatCount(value: number) {
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}M`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(1)}k`;
  }
  return String(value);
}

export function telemetryValue(point: TelemetryPoint) {
  const payload = isRecord(point.payload) ? point.payload : null;
  return (
    getNumber(payload, "cpu_percent") ??
    getNumber(payload, "temperature_c") ??
    getNumber(payload, "disk_used_percent") ??
    getNumber(payload, "memory_mb") ??
    0
  );
}

export function toDeviceRows(devices: Device[], streams: StreamDefinition[], telemetry: TelemetryPoint[]) {
  const latestTelemetry = new Map<string, TelemetryPoint>();
  telemetry.forEach((point) => {
    if (!latestTelemetry.has(point.device_id)) {
      latestTelemetry.set(point.device_id, point);
    }
  });
  const defaultStream = streams[0]?.name ?? SYSTEM_STREAM;

  return devices.map<DeviceRow>((device) => {
    const metadata = isRecord(device.metadata) ? device.metadata : null;
    const shadow = isRecord(device.latest_shadow) ? device.latest_shadow : null;
    const firmware = nestedRecord(shadow, "firmware") ?? nestedRecord(metadata, "firmware");
    const agent = nestedRecord(shadow, "agent") ?? nestedRecord(metadata, "agent");
    const latest = latestTelemetry.get(device.id);
    const payload = latest && isRecord(latest.payload) ? latest.payload : null;
    const firmwareVersion =
      getString(firmware, "main") ??
      getString(firmware, "version") ??
      getString(agent, "version") ??
      getString(metadata, "firmware") ??
      "-";
    const shadowLabel =
      getString(shadow, "state") ??
      getString(shadow, "status") ??
      getString(shadow, "mode") ??
      (shadow && Object.keys(shadow).length > 0 ? "reported" : "empty");

    return {
      id: device.id,
      name: device.name,
      status: normalizeDeviceStatus(device.status),
      stream: latest?.stream ?? defaultStream,
      firmware: firmwareVersion,
      lastSeen: formatRelativeTime(device.last_seen_at ?? null),
      rssi: getNumber(payload, "rssi_dbm") ?? getNumber(metadata, "rssi_dbm"),
      shadow: shadowLabel,
    };
  });
}

export function toStreamSummaries(streams: StreamDefinition[], telemetry: TelemetryPoint[]): StreamSummary[] {
  const counts = new Map<string, number>();
  telemetry.forEach((point) => {
    counts.set(point.stream, (counts.get(point.stream) ?? 0) + 1);
  });
  const streamNames = new Set([...streams.map((stream) => stream.name), ...counts.keys()]);
  return [...streamNames].map((name) => ({
    name,
    rows: formatCount(counts.get(name) ?? 0),
    p95: "local query",
    retention: name === SYSTEM_STREAM ? "90d" : "180d",
  }));
}

export function toActionSummaries(actions: Action[], devices: Device[], limit?: number): ActionSummary[] {
  const devicesById = new Map(devices.map((device) => [device.id, device.name]));
  const visibleActions = typeof limit === "number" ? actions.slice(0, limit) : actions;
  return visibleActions.map((action) => {
    const state = humanizeEnum(action.state);
    const target =
      action.device_ids.length === 1
        ? devicesById.get(action.device_ids[0]) ?? action.device_ids[0]
        : `${action.device_ids.length} devices`;
    return {
      id: action.id,
      name: action.name,
      target,
      progress: action.progress,
      state,
    };
  });
}

export function toAlertSummaries(alerts: AlertRule[], devices: Device[]): AlertSummary[] {
  const offlineCount = devices.filter((device) => normalizeDeviceStatus(device.status) === "offline").length;
  return alerts.map((alert) => {
    const expression = isRecord(alert.expression) ? alert.expression : null;
    const kind = humanizeEnum(alert.kind);
    const state = kind === "offline" && offlineCount > 0 ? "firing" : "quiet";
    return {
      id: alert.id,
      name: alert.name,
      kind,
      state,
      target: getString(expression, "stream") ?? getString(expression, "window") ?? "project scope",
    };
  });
}

export function toDashboardStats(
  projectData: ProjectData,
  deviceRows: DeviceRow[],
  alertSummaries: AlertSummary[],
): DashboardStats {
  const deviceStatusCounts: Record<DeviceStatus, number> = {
    online: 0,
    offline: 0,
    disabled: 0,
    provisioned: 0,
  };
  deviceRows.forEach((device) => {
    deviceStatusCounts[device.status] += 1;
  });
  const openActions = projectData.actions.filter((action) => !isTerminalAction(action)).length;
  const firingAlerts = alertSummaries.filter((alert) => alert.state === "firing").length;
  const activeFirmwareCount = projectData.firmware.filter((artifact) => artifact.active).length;
  const activeRolloutCount = projectData.firmwareRollouts.filter(isActiveRollout).length;

  return {
    totalDevices: deviceRows.length,
    onlineDevices: deviceStatusCounts.online,
    offlineDevices: deviceStatusCounts.offline,
    disabledDevices: deviceStatusCounts.disabled,
    provisionedDevices: deviceStatusCounts.provisioned,
    onlinePercent: deviceRows.length === 0 ? 0 : Math.round((deviceStatusCounts.online / deviceRows.length) * 100),
    telemetryRows: projectData.telemetry.length,
    streamCount: projectData.streams.length,
    openActions,
    totalActions: projectData.actions.length,
    firingAlerts,
    totalAlerts: projectData.alerts.length,
    firmwareCount: projectData.firmware.length,
    activeFirmwareCount,
    rolloutCount: projectData.firmwareRollouts.length,
    activeRolloutCount,
    deviceStatusCounts,
  };
}
