"use client";

import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Bell, Boxes, RadioTower, SunMoon, Wifi, Zap } from "lucide-react";
import { ActionQueuePanel, AlertPanel } from "@/components/action-alert-panels";
import { DeviceAgentPanel } from "@/components/device-agent-panel";
import { DeviceTable } from "@/components/device-table";
import { MetricStrip } from "@/components/metric-strip";
import { ProjectHeader } from "@/components/project-header";
import { Sidebar } from "@/components/sidebar";
import { TelemetryPanel } from "@/components/telemetry-panel";
import {
  ExcaliburApiError,
  createExcaliburApi,
  type Action,
  type AlertRule,
  type AuditLog,
  type AuthResponse,
  type Device,
  type DeviceConfig,
  type FirmwareArtifact,
  type JsonValue,
  type Org,
  type Project,
  type StreamDefinition,
  type TelemetryPoint,
} from "@/lib/api";
import type {
  ActionSummary,
  AlertSummary,
  DeviceRow,
  DeviceStatus,
  MetricItem,
  StreamSummary,
} from "@/lib/data";
import { commandStatusTopic, commandTopic, shadowTopic, telemetryTopic } from "@/lib/protocol";

type Session = {
  token: string;
  refreshToken: string;
  expiresAt: string;
  refreshExpiresAt: string;
  userId: string;
};

type ThemeMode = "dark" | "light";

type Workspace = {
  org: Org;
  project: Project;
};

type ProjectData = {
  devices: Device[];
  streams: StreamDefinition[];
  telemetry: TelemetryPoint[];
  actions: Action[];
  firmware: FirmwareArtifact[];
  alerts: AlertRule[];
  audit: AuditLog[];
};

type Api = ReturnType<typeof createExcaliburApi>;

const SESSION_KEY = "excalibur.console.session.v1";
const API_BASE_KEY = "excalibur.console.apiBaseUrl.v1";
const THEME_KEY = "excalibur.console.theme.v1";
const DEFAULT_API_BASE_URL = process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080";
const SYSTEM_STREAM = "device_agent_system_stats";
const DEFAULT_SHA256 = "a".repeat(64);
const SESSION_REFRESH_SKEW_MS = 60_000;

const emptyProjectData: ProjectData = {
  devices: [],
  streams: [],
  telemetry: [],
  actions: [],
  firmware: [],
  alerts: [],
  audit: [],
};

function formatError(error: unknown) {
  if (error instanceof ExcaliburApiError) {
    return `${error.status}: ${error.message}`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "Unknown error";
}

function isThemeMode(value: string | null | undefined): value is ThemeMode {
  return value === "dark" || value === "light";
}

function isStoredSession(value: unknown): value is Session {
  if (!value || typeof value !== "object") {
    return false;
  }
  const session = value as Partial<Record<keyof Session, unknown>>;
  return (
    typeof session.token === "string" &&
    typeof session.refreshToken === "string" &&
    typeof session.expiresAt === "string" &&
    typeof session.refreshExpiresAt === "string" &&
    typeof session.userId === "string"
  );
}

function sessionFromAuth(auth: AuthResponse): Session {
  return {
    token: auth.token,
    refreshToken: auth.refresh_token,
    expiresAt: auth.expires_at,
    refreshExpiresAt: auth.refresh_expires_at,
    userId: auth.user_id,
  };
}

function expiresBefore(iso: string, cutoffMs: number) {
  const expiresAt = Date.parse(iso);
  return !Number.isFinite(expiresAt) || expiresAt <= cutoffMs;
}

function isRecord(value: JsonValue | undefined): value is Record<string, JsonValue> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function getString(record: Record<string, JsonValue> | null, key: string) {
  const value = record?.[key];
  return typeof value === "string" ? value : null;
}

function getNumber(record: Record<string, JsonValue> | null, key: string) {
  const value = record?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function nestedRecord(record: Record<string, JsonValue> | null, key: string) {
  const value = record?.[key];
  return isRecord(value) ? value : null;
}

function humanizeEnum(value: string) {
  return value
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/_/g, " ")
    .trim()
    .toLowerCase();
}

function normalizeDeviceStatus(status: string): DeviceStatus {
  const value = humanizeEnum(status);
  if (value === "online" || value === "offline" || value === "disabled") {
    return value;
  }
  return "provisioned";
}

function isTerminalAction(action: Action) {
  const state = humanizeEnum(action.state);
  return state === "completed" || state === "failed" || state === "cancelled" || state === "timed out";
}

function formatRelativeTime(iso: string | null) {
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

function formatCount(value: number) {
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}M`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(1)}k`;
  }
  return String(value);
}

function randomUuid() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  const suffix = Date.now().toString(16).padStart(12, "0").slice(-12);
  return `00000000-0000-4000-8000-${suffix}`;
}

function telemetryValue(point: TelemetryPoint) {
  const payload = isRecord(point.payload) ? point.payload : null;
  return (
    getNumber(payload, "cpu_percent") ??
    getNumber(payload, "temperature_c") ??
    getNumber(payload, "disk_used_percent") ??
    getNumber(payload, "memory_mb") ??
    0
  );
}

function toDeviceRows(devices: Device[], streams: StreamDefinition[], telemetry: TelemetryPoint[]) {
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
      lastSeen: formatRelativeTime(device.last_seen_at),
      rssi: getNumber(payload, "rssi_dbm") ?? getNumber(metadata, "rssi_dbm"),
      shadow: shadowLabel,
    };
  });
}

function toStreamSummaries(streams: StreamDefinition[], telemetry: TelemetryPoint[]): StreamSummary[] {
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

function toActionSummaries(actions: Action[], devices: Device[]): ActionSummary[] {
  const devicesById = new Map(devices.map((device) => [device.id, device.name]));
  return actions.slice(0, 6).map((action) => {
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

function toAlertSummaries(alerts: AlertRule[], devices: Device[]): AlertSummary[] {
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

function downloadJsonFile(filename: string, value: unknown) {
  const blob = new Blob([JSON.stringify(value, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

async function ignoreConflict<T>(work: () => Promise<T>) {
  try {
    return await work();
  } catch (error) {
    if (error instanceof ExcaliburApiError && error.status === 409) {
      return null;
    }
    throw error;
  }
}

async function ensureOrg(api: Api, userId: string) {
  const existing = await api.listOrgs();
  if (existing[0]) {
    return existing[0];
  }
  const slug = `excalibur-${userId.slice(0, 8).toLowerCase()}`;
  const created = await ignoreConflict(() =>
    api.createOrg({
      name: "Excalibur Demo Org",
      slug,
    }),
  );
  if (created) {
    return created;
  }
  const retry = await api.listOrgs();
  if (!retry[0]) {
    throw new Error("workspace org could not be initialized");
  }
  return retry[0];
}

async function ensureProject(api: Api, org: Org) {
  const existing = await api.listProjects(org.id);
  if (existing[0]) {
    return existing[0];
  }
  const created = await ignoreConflict(() =>
    api.createProject({
      org_id: org.id,
      name: "Factory Line",
      slug: "factory-line",
    }),
  );
  if (created) {
    return created;
  }
  const retry = await api.listProjects(org.id);
  if (!retry[0]) {
    throw new Error("workspace project could not be initialized");
  }
  return retry[0];
}

async function ensureDefaultControlPlane(api: Api, projectId: string) {
  const [streams, alerts, dashboards] = await Promise.all([
    api.listStreams(projectId),
    api.listAlerts(projectId),
    api.listDashboards(projectId),
  ]);

  if (!streams.some((stream) => stream.name === SYSTEM_STREAM)) {
    await ignoreConflict(() =>
      api.createStream({
        project_id: projectId,
        name: SYSTEM_STREAM,
        fields: [
          { name: "cpu_percent", field_type: "Float", required: true },
          { name: "memory_mb", field_type: "Float", required: true },
          { name: "disk_used_percent", field_type: "Float", required: false },
          { name: "rssi_dbm", field_type: "Integer", required: false },
        ],
      }),
    );
  }

  if (alerts.length === 0) {
    await ignoreConflict(() =>
      api.createAlert({
        project_id: projectId,
        name: "offline > 10m",
        kind: "Offline",
        expression: { window: "10m", stream: SYSTEM_STREAM },
      }),
    );
  }

  if (dashboards.length === 0) {
    await ignoreConflict(() =>
      api.createDashboard({
        project_id: projectId,
        name: "Fleet overview",
        layout: {
          panels: [
            { type: "metric", source: "devices.online" },
            { type: "timeseries", stream: SYSTEM_STREAM, field: "cpu_percent" },
          ],
        },
      }),
    );
  }
}

async function ensureFirmwareArtifact(api: Api, projectId: string) {
  const existing = await api.listFirmware(projectId);
  if (existing[0]) {
    return existing[0];
  }
  const created = await ignoreConflict(() =>
    api.createFirmware({
      project_id: projectId,
      component: "main",
      version: "1.0.0",
      object_key: "firmware/main/1.0.0/excalibur-agent.bin",
      sha256: DEFAULT_SHA256,
      size_bytes: 1_048_576,
    }),
  );
  if (created) {
    return created;
  }
  const retry = await api.listFirmware(projectId);
  if (!retry[0]) {
    throw new Error("firmware artifact could not be initialized");
  }
  return retry[0];
}

function makeSampleTelemetry(sequence: number) {
  const cpu = 32 + Math.round(Math.random() * 45);
  return {
    sequence,
    timestamp: new Date().toISOString(),
    cpu_percent: cpu,
    memory_mb: 420 + Math.round(Math.random() * 380),
    disk_used_percent: 41 + Math.round(Math.random() * 18),
    rssi_dbm: -58 - Math.round(Math.random() * 18),
  };
}

export function ConsoleApp() {
  const [theme, setTheme] = useState<ThemeMode>(() => {
    if (typeof window === "undefined") {
      return "dark";
    }
    const savedTheme = window.localStorage.getItem(THEME_KEY);
    return isThemeMode(savedTheme) ? savedTheme : "dark";
  });
  const [apiBaseUrl, setApiBaseUrl] = useState(DEFAULT_API_BASE_URL);
  const [authMode, setAuthMode] = useState<"login" | "register">("register");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [session, setSession] = useState<Session | null>(null);
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [projectData, setProjectData] = useState<ProjectData>(emptyProjectData);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | undefined>();
  const [devAuthConfig, setDevAuthConfig] = useState<DeviceConfig | null>(null);
  const [search, setSearch] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const initializedSessionKey = useRef<string | null>(null);

  const clearSession = useCallback(() => {
    window.localStorage.removeItem(SESSION_KEY);
    setSession(null);
  }, []);

  const persistSession = useCallback((nextSession: Session) => {
    window.localStorage.setItem(SESSION_KEY, JSON.stringify(nextSession));
    setSession(nextSession);
    return nextSession;
  }, []);

  const getApiForSession = useCallback(
    async (activeSession: Session) => {
      let usableSession = activeSession;
      if (expiresBefore(activeSession.expiresAt, Date.now() + SESSION_REFRESH_SKEW_MS)) {
        if (expiresBefore(activeSession.refreshExpiresAt, Date.now())) {
          clearSession();
          throw new Error("Session expired");
        }
        try {
          const authApi = createExcaliburApi({ baseUrl: apiBaseUrl });
          const auth = await authApi.refreshSession({ refresh_token: activeSession.refreshToken });
          usableSession = persistSession(sessionFromAuth(auth));
        } catch (refreshError) {
          clearSession();
          throw refreshError;
        }
      }
      return createExcaliburApi({ baseUrl: apiBaseUrl, token: usableSession.token });
    },
    [apiBaseUrl, clearSession, persistSession],
  );

  const loadProjectData = useCallback(async (api: Api, orgId: string, projectId: string) => {
    const [devices, streams, telemetry, actions, firmware, alerts, audit] = await Promise.all([
      api.listDevices(projectId),
      api.listStreams(projectId),
      api.queryTelemetry({ projectId, limit: 200 }),
      api.listActions(projectId),
      api.listFirmware(projectId),
      api.listAlerts(projectId),
      api.listAudit(orgId, projectId),
    ]);

    setProjectData({ devices, streams, telemetry, actions, firmware, alerts, audit });
    setSelectedDeviceId((current) =>
      current && devices.some((device) => device.id === current) ? current : devices[0]?.id,
    );
  }, []);

  const initializeWorkspace = useCallback(
    async (activeSession: Session) => {
      setBusy(true);
      setError(null);
      setNotice("Loading workspace");
      try {
        const api = await getApiForSession(activeSession);
        const org = await ensureOrg(api, activeSession.userId);
        const project = await ensureProject(api, org);
        await ensureDefaultControlPlane(api, project.id);
        setWorkspace({ org, project });
        await loadProjectData(api, org.id, project.id);
        setNotice("Workspace ready");
        return true;
      } catch (loadError) {
        setError(formatError(loadError));
        return false;
      } finally {
        setBusy(false);
      }
    },
    [getApiForSession, loadProjectData],
  );

  useEffect(() => {
    const savedApiBaseUrl = window.localStorage.getItem(API_BASE_KEY);
    const savedSession = window.localStorage.getItem(SESSION_KEY);
    if (savedApiBaseUrl) {
      setApiBaseUrl(savedApiBaseUrl);
    }
    if (savedSession) {
      try {
        const parsedSession: unknown = JSON.parse(savedSession);
        if (isStoredSession(parsedSession)) {
          setSession(parsedSession);
        } else {
          window.localStorage.removeItem(SESSION_KEY);
        }
      } catch {
        window.localStorage.removeItem(SESSION_KEY);
      }
    }
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    window.localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  const handleToggleTheme = useCallback(() => {
    setTheme((current) => (current === "dark" ? "light" : "dark"));
  }, []);

  useEffect(() => {
    if (session) {
      const sessionKey = `${apiBaseUrl}:${session.token}`;
      if (initializedSessionKey.current === sessionKey) {
        return;
      }
      initializedSessionKey.current = sessionKey;
      void initializeWorkspace(session).then((loaded) => {
        if (!loaded && initializedSessionKey.current === sessionKey) {
          initializedSessionKey.current = null;
        }
      });
    } else {
      initializedSessionKey.current = null;
      setWorkspace(null);
      setProjectData(emptyProjectData);
      setSelectedDeviceId(undefined);
      setDevAuthConfig(null);
    }
  }, [apiBaseUrl, initializeWorkspace, session]);

  const runProjectMutation = useCallback(
    async (success: string, work: (api: Api, workspace: Workspace) => Promise<void>) => {
      if (!session || !workspace) {
        return;
      }
      setBusy(true);
      setError(null);
      try {
        const api = await getApiForSession(session);
        await work(api, workspace);
        await loadProjectData(api, workspace.org.id, workspace.project.id);
        setNotice(success);
      } catch (mutationError) {
        setError(formatError(mutationError));
      } finally {
        setBusy(false);
      }
    },
    [getApiForSession, loadProjectData, session, workspace],
  );

  const selectedDevice = useMemo(
    () => projectData.devices.find((device) => device.id === selectedDeviceId),
    [projectData.devices, selectedDeviceId],
  );

  const deviceRows = useMemo(
    () => toDeviceRows(projectData.devices, projectData.streams, projectData.telemetry),
    [projectData.devices, projectData.streams, projectData.telemetry],
  );

  const filteredDeviceRows = useMemo(() => {
    const value = search.trim().toLowerCase();
    if (!value) {
      return deviceRows;
    }
    return deviceRows.filter((device) =>
      [device.name, device.id, device.status, device.stream, device.firmware, device.shadow]
        .join(" ")
        .toLowerCase()
        .includes(value),
    );
  }, [deviceRows, search]);

  const selectedDeviceRow = deviceRows.find((device) => device.id === selectedDeviceId);
  const telemetryValues = useMemo(
    () =>
      projectData.telemetry
        .filter((point) => !selectedDeviceId || point.device_id === selectedDeviceId)
        .slice()
        .reverse()
        .map(telemetryValue),
    [projectData.telemetry, selectedDeviceId],
  );
  const streamSummaries = useMemo(
    () => toStreamSummaries(projectData.streams, projectData.telemetry),
    [projectData.streams, projectData.telemetry],
  );
  const actionSummaries = useMemo(
    () => toActionSummaries(projectData.actions, projectData.devices),
    [projectData.actions, projectData.devices],
  );
  const alertSummaries = useMemo(
    () => toAlertSummaries(projectData.alerts, projectData.devices),
    [projectData.alerts, projectData.devices],
  );
  const metrics = useMemo<MetricItem[]>(() => {
    const online = deviceRows.filter((device) => device.status === "online").length;
    const openActions = projectData.actions.filter((action) => !isTerminalAction(action)).length;
    const firingAlerts = alertSummaries.filter((alert) => alert.state === "firing").length;
    return [
      {
        label: "Connected devices",
        value: `${online}/${deviceRows.length}`,
        delta: deviceRows.length === 0 ? "no devices" : `${Math.round((online / deviceRows.length) * 100)}% online`,
        tone: "teal",
        icon: Wifi,
      },
      {
        label: "Telemetry rows",
        value: formatCount(projectData.telemetry.length),
        delta: `${streamSummaries.length} streams`,
        tone: "signal",
        icon: RadioTower,
      },
      {
        label: "Open actions",
        value: String(openActions),
        delta: `${projectData.actions.length} total`,
        tone: "amber",
        icon: Zap,
      },
      {
        label: "Alert pressure",
        value: String(firingAlerts),
        delta: `${projectData.alerts.length} rules`,
        tone: firingAlerts > 0 ? "danger" : "teal",
        icon: Bell,
      },
    ];
  }, [alertSummaries, deviceRows, projectData.actions, projectData.alerts.length, projectData.telemetry.length, streamSummaries.length]);

  const handleAuthenticate = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const api = createExcaliburApi({ baseUrl: apiBaseUrl });
      const auth =
        authMode === "register"
          ? await api.register({
              email,
              password,
              display_name: displayName.trim() || email,
            })
          : await api.login({ email, password });
      const nextSession = sessionFromAuth(auth);
      window.localStorage.setItem(API_BASE_KEY, apiBaseUrl);
      persistSession(nextSession);
      setNotice("Signed in");
    } catch (authError) {
      setError(formatError(authError));
    } finally {
      setBusy(false);
    }
  };

  const handleLogout = useCallback(() => {
    const activeSession = session;
    setBusy(true);
    setNotice(null);
    setError(null);
    void (async () => {
      try {
        if (activeSession) {
          const api = await getApiForSession(activeSession);
          await api.logout();
        }
      } catch (logoutError) {
        setError(formatError(logoutError));
      } finally {
        clearSession();
        setBusy(false);
      }
    })();
  }, [clearSession, getApiForSession, session]);

  const handleRefresh = useCallback(() => {
    if (session && !workspace) {
      initializedSessionKey.current = null;
      void initializeWorkspace(session);
      return;
    }
    void runProjectMutation("Refreshed", async () => {});
  }, [initializeWorkspace, runProjectMutation, session, workspace]);

  const handleCreateDevice = useCallback(() => {
    void runProjectMutation("Device created", async (api, activeWorkspace) => {
      const index = projectData.devices.length + 1;
      const device = await api.createDevice({
        project_id: activeWorkspace.project.id,
        name: `linux-edge-${String(index).padStart(3, "0")}`,
        metadata: {
          fleet: "factory-line",
          agent: { version: "device-agent 1.0.0" },
        },
      });
      setSelectedDeviceId(device.id);
    });
  }, [projectData.devices.length, runProjectMutation]);

  const handleDownloadDevAuth = useCallback(
    (deviceId?: string) => {
      const targetDeviceId = deviceId ?? selectedDeviceId;
      if (!targetDeviceId) {
        return;
      }
      void runProjectMutation("Dev auth JSON issued", async (api, activeWorkspace) => {
        const config = await api.provisionDevAuth(targetDeviceId, activeWorkspace.project.id);
        setDevAuthConfig(config);
        downloadJsonFile(`excalibur-${targetDeviceId}-dev-auth.json`, config);
      });
    },
    [runProjectMutation, selectedDeviceId],
  );

  const handleIngestSample = useCallback(
    (deviceId?: string) => {
      const targetDevice = projectData.devices.find((device) => device.id === (deviceId ?? selectedDeviceId));
      if (!targetDevice || !workspace) {
        return;
      }
      void runProjectMutation("Sample telemetry ingested", async (api, activeWorkspace) => {
        const sequence = Date.now() * 1000 + Math.round(Math.random() * 999);
        await api.ingestTelemetry({
          topic: shadowTopic(activeWorkspace.project.id, targetDevice.id),
          payload: {
            state: "nominal",
            agent: { version: "device-agent 1.0.0" },
            firmware: { main: "main/1.0.0" },
          },
        });
        await api.ingestTelemetry({
          topic: telemetryTopic(activeWorkspace.project.id, targetDevice.id, SYSTEM_STREAM),
          payload: [makeSampleTelemetry(sequence)],
        });
      });
    },
    [projectData.devices, runProjectMutation, selectedDeviceId, workspace],
  );

  const handleCreateDiagnostics = useCallback(() => {
    if (!selectedDeviceId) {
      return;
    }
    void runProjectMutation("Diagnostics action queued", async (api, activeWorkspace) => {
      const action = await api.createAction({
        project_id: activeWorkspace.project.id,
        device_ids: [selectedDeviceId],
        name: "diagnostics.collect",
        payload: {
          session_id: randomUuid(),
          paths: ["/var/log/excalibur-agent"],
          include_logs: true,
          include_system_stats: true,
          upload_url: "http://localhost:9000/excalibur-diagnostics/session.tar.zst?dev=1",
        },
      });
      await api.updateActionStatus(action.id, {
        project_id: activeWorkspace.project.id,
        device_id: selectedDeviceId,
        state: "Running",
        progress: 35,
        errors: [],
      });
    });
  }, [runProjectMutation, selectedDeviceId]);

  const handleCreateOta = useCallback(() => {
    if (!selectedDeviceId) {
      return;
    }
    void runProjectMutation("OTA action queued", async (api, activeWorkspace) => {
      const firmware = await ensureFirmwareArtifact(api, activeWorkspace.project.id);
      const action = await api.createAction({
        project_id: activeWorkspace.project.id,
        device_ids: [selectedDeviceId],
        name: "ota.install",
        payload: {
          firmware_id: firmware.id,
          component: firmware.component,
          version: firmware.version,
          signed_url: `http://localhost:9000/excalibur-firmware/${firmware.object_key}?dev=1`,
          sha256: firmware.sha256,
          signature: "ed25519:local-dev",
          size_bytes: firmware.size_bytes,
        },
      });
      await api.updateActionStatus(action.id, {
        project_id: activeWorkspace.project.id,
        device_id: selectedDeviceId,
        state: "Running",
        progress: 20,
        errors: [],
      });
    });
  }, [runProjectMutation, selectedDeviceId]);

  const handleCompleteLatest = useCallback(() => {
    const action = projectData.actions.find(
      (candidate) =>
        (!selectedDeviceId || candidate.device_ids.includes(selectedDeviceId)) && !isTerminalAction(candidate),
    );
    const targetDeviceId = selectedDeviceId ?? action?.device_ids[0];
    if (!action || !targetDeviceId) {
      return;
    }
    void runProjectMutation("Action completed", async (api, activeWorkspace) => {
      await api.updateActionStatus(action.id, {
        project_id: activeWorkspace.project.id,
        device_id: targetDeviceId,
        state: "Completed",
        progress: 100,
        errors: [],
      });
    });
  }, [projectData.actions, runProjectMutation, selectedDeviceId]);

  const handleCreateDefaultAlert = useCallback(() => {
    void runProjectMutation("Alert rule created", async (api, activeWorkspace) => {
      await api.createAlert({
        project_id: activeWorkspace.project.id,
        name: `cpu > 85 ${projectData.alerts.length + 1}`,
        kind: "Threshold",
        expression: { stream: SYSTEM_STREAM, field: "cpu_percent", op: ">", value: 85 },
      });
    });
  }, [projectData.alerts.length, runProjectMutation]);

  const handleBootstrapDemo = useCallback(() => {
    void runProjectMutation("Closed-loop demo data created", async (api, activeWorkspace) => {
      await ensureDefaultControlPlane(api, activeWorkspace.project.id);
      const devices = await api.listDevices(activeWorkspace.project.id);
      const device =
        devices[0] ??
        (await api.createDevice({
          project_id: activeWorkspace.project.id,
          name: "linux-edge-001",
          metadata: {
            fleet: "factory-line",
            agent: { version: "device-agent 1.0.0" },
          },
        }));
      setSelectedDeviceId(device.id);
      await ensureFirmwareArtifact(api, activeWorkspace.project.id);
      const sequence = Date.now() * 1000 + Math.round(Math.random() * 999);
      await api.ingestTelemetry({
        topic: shadowTopic(activeWorkspace.project.id, device.id),
        payload: {
          state: "nominal",
          mode: "production",
          agent: { version: "device-agent 1.0.0" },
          firmware: { main: "main/1.0.0" },
        },
      });
      await api.ingestTelemetry({
        topic: telemetryTopic(activeWorkspace.project.id, device.id, SYSTEM_STREAM),
        payload: [makeSampleTelemetry(sequence)],
      });
      const diagnostics = await api.createAction({
        project_id: activeWorkspace.project.id,
        device_ids: [device.id],
        name: "diagnostics.collect",
        payload: {
          session_id: randomUuid(),
          paths: ["/var/log/excalibur-agent"],
          include_logs: true,
          include_system_stats: true,
          upload_url: "http://localhost:9000/excalibur-diagnostics/bootstrap.tar.zst?dev=1",
        },
      });
      await api.updateActionStatus(diagnostics.id, {
        project_id: activeWorkspace.project.id,
        device_id: device.id,
        state: "Completed",
        progress: 100,
        errors: [],
      });
    });
  }, [runProjectMutation]);

  const protocolDevice = selectedDevice ?? projectData.devices[0];

  if (!session) {
    return (
      <main className="grid min-h-screen place-items-center bg-paper px-4 py-10">
        <form className="w-full max-w-md rounded-md border border-line bg-panel p-5 shadow-panel" onSubmit={handleAuthenticate}>
          <div className="flex items-start justify-between gap-3">
            <div className="flex items-center gap-3">
              <div className="grid h-10 w-10 place-items-center rounded-md bg-brand text-ink">
                <Boxes className="h-5 w-5" aria-hidden="true" />
              </div>
              <div>
                <h1 className="text-lg font-semibold text-ink">Excalibur Console</h1>
                <p className="text-sm text-muted">Control plane sign-in</p>
              </div>
            </div>
            <button
              className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md border border-line bg-elevated text-muted transition hover:bg-line hover:text-ink"
              type="button"
              aria-label="Toggle theme"
              title="Toggle theme"
              onClick={handleToggleTheme}
            >
              <SunMoon className="h-4 w-4" aria-hidden="true" />
            </button>
          </div>

          <div className="mt-5 grid grid-cols-2 gap-2 rounded-md bg-rail p-1">
            <button
              className={`h-9 rounded-sm text-sm font-medium transition ${authMode === "register" ? "bg-elevated text-ink" : "text-muted hover:text-ink"}`}
              type="button"
              onClick={() => setAuthMode("register")}
            >
              Register
            </button>
            <button
              className={`h-9 rounded-sm text-sm font-medium transition ${authMode === "login" ? "bg-elevated text-ink" : "text-muted hover:text-ink"}`}
              type="button"
              onClick={() => setAuthMode("login")}
            >
              Login
            </button>
          </div>

          <label className="mt-4 block text-sm font-medium text-muted">
            API base URL
            <input
              className="mt-1 h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
              value={apiBaseUrl}
              onChange={(event) => setApiBaseUrl(event.target.value)}
              type="url"
            />
          </label>
          <label className="mt-3 block text-sm font-medium text-muted">
            Email
            <input
              className="mt-1 h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              type="email"
              required
            />
          </label>
          {authMode === "register" ? (
            <label className="mt-3 block text-sm font-medium text-muted">
              Display name
              <input
                className="mt-1 h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
                value={displayName}
                onChange={(event) => setDisplayName(event.target.value)}
                type="text"
              />
            </label>
          ) : null}
          <label className="mt-3 block text-sm font-medium text-muted">
            Password
            <input
              className="mt-1 h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              type="password"
              minLength={12}
              required
            />
          </label>

          {error ? <p className="mt-3 rounded-sm bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p> : null}

          <button
            className="mt-5 h-10 w-full rounded-md bg-brand text-sm font-semibold text-ink transition hover:bg-brand-hover disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint"
            type="submit"
            disabled={busy}
          >
            {busy ? "Working..." : authMode === "register" ? "Create account" : "Sign in"}
          </button>
        </form>
      </main>
    );
  }

  return (
    <main className="min-h-screen bg-paper pb-20 text-ink lg:flex lg:pb-0">
      <Sidebar />
      <div className="min-w-0 flex-1">
        <ProjectHeader
          orgName={workspace?.org.name ?? "Loading org"}
          projectName={workspace?.project.name ?? "Loading project"}
          apiBaseUrl={apiBaseUrl}
          search={search}
          busy={busy}
          onSearch={setSearch}
          onToggleTheme={handleToggleTheme}
          onRefresh={handleRefresh}
          onBootstrapDemo={handleBootstrapDemo}
          onLogout={handleLogout}
        />
        <div className="space-y-5 px-4 py-5 md:px-6">
          {error || notice ? (
            <div
              className={`rounded-md border px-4 py-3 text-sm ${
                error ? "border-danger/25 bg-danger/10 text-danger" : "border-success/25 bg-success/10 text-success"
              }`}
            >
              {error ?? notice}
            </div>
          ) : null}

          <MetricStrip metrics={metrics} />

          <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
            <div className="min-w-0 space-y-5">
              <TelemetryPanel
                values={telemetryValues}
                streams={streamSummaries}
                rowRateLabel={`${formatCount(projectData.telemetry.length)} rows`}
                selectedDeviceName={selectedDeviceRow?.name}
                busy={busy}
                onIngestSample={() => handleIngestSample()}
              />
              <DeviceTable
                data={filteredDeviceRows}
                selectedDeviceId={selectedDeviceId}
                busy={busy}
                onCreateDevice={handleCreateDevice}
                onSelectDevice={setSelectedDeviceId}
                onDownloadDevAuth={handleDownloadDevAuth}
                onIngestSample={handleIngestSample}
              />
              <DeviceAgentPanel
                device={selectedDeviceRow}
                projectId={workspace?.project.id}
                devAuthConfig={devAuthConfig}
                busy={busy}
                onDownloadDevAuth={() => handleDownloadDevAuth()}
                onIngestSample={() => handleIngestSample()}
                onCreateDiagnostics={handleCreateDiagnostics}
                onCreateOta={handleCreateOta}
              />
            </div>

            <aside className="min-w-0 space-y-5">
              <ActionQueuePanel
                actions={actionSummaries}
                busy={busy}
                canRunDeviceAction={Boolean(selectedDeviceId)}
                onCreateDiagnostics={handleCreateDiagnostics}
                onCreateOta={handleCreateOta}
                onCompleteLatest={handleCompleteLatest}
              />
              <AlertPanel rules={alertSummaries} busy={busy} onCreateDefault={handleCreateDefaultAlert} />
              <section className="panel-in rounded-md border border-line bg-rail p-4 text-ink">
                <h2 className="text-base font-semibold">Protocol</h2>
                <div className="mt-3 space-y-3 text-xs text-muted">
                  {protocolDevice && workspace ? (
                    [
                      ["telemetry publish", telemetryTopic(workspace.project.id, protocolDevice.id, SYSTEM_STREAM)],
                      ["shadow publish", shadowTopic(workspace.project.id, protocolDevice.id)],
                      ["commands subscribe", commandTopic(workspace.project.id, protocolDevice.id)],
                      ["command status", commandStatusTopic(workspace.project.id, protocolDevice.id)],
                    ].map(([label, topic]) => (
                      <div key={label}>
                        <p className="mb-1 text-faint">{label}</p>
                        <code className="block break-all rounded-sm bg-elevated p-2 text-ink">{topic}</code>
                      </div>
                    ))
                  ) : (
                    <p className="text-muted">No device selected.</p>
                  )}
                </div>
              </section>
              <section className="panel-in rounded-md border border-line bg-panel">
                <div className="border-b border-line px-4 py-3">
                  <h2 className="text-base font-semibold text-ink">Audit</h2>
                  <p className="text-sm text-muted">Recent scoped control-plane writes.</p>
                </div>
                <div className="divide-y divide-line">
                  {projectData.audit.slice(0, 6).map((entry) => (
                    <article key={entry.id} className="px-4 py-3">
                      <p className="truncate text-sm font-medium text-ink">{entry.action}</p>
                      <p className="truncate text-xs text-faint">{entry.resource}</p>
                    </article>
                  ))}
                  {projectData.audit.length === 0 ? (
                    <div className="px-4 py-6 text-center text-sm text-muted">No audit entries yet.</div>
                  ) : null}
                </div>
              </section>
            </aside>
          </div>
        </div>
      </div>
    </main>
  );
}
