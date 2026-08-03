"use client";

import {
  createContext,
  type FormEvent,
  type ReactNode,
  type SetStateAction,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Bell, RadioTower, Wifi, Zap } from "lucide-react";
import {
  ExcaliburApiError,
  createExcaliburApi,
  type ApiKey,
  type AuthResponse,
  type DeviceConfig,
  type JsonValue,
  type Org,
  type Project,
  type ProjectFeature,
  type RemoteShellSession,
} from "@/lib/api";
import {
  SYSTEM_STREAM,
  emptyProjectData,
  formatCount,
  isRecord,
  isTerminalAction,
  telemetryValue,
  toActionSummaries,
  toAlertSummaries,
  toDashboardStats,
  toDeviceRows,
  toStreamSummaries,
  type DashboardStats,
  type ProjectData,
} from "@/lib/console-model";
import type { ActionSummary, AlertSummary, DeviceRow, MetricItem, StreamSummary } from "@/lib/data";
import { commandStatusTopic, commandTopic, shadowTopic, telemetryTopic } from "@/lib/protocol";
import { getApiKeyScopePreset, slugifyWorkspaceName } from "@/lib/workspace-management";
import type { ApiKeyCreateInput } from "@/components/workspace-management-panels";

export type Session = {
  expiresAt: string;
  refreshExpiresAt: string;
  userId: string;
};

export type ThemeMode = "dark" | "light";

export type Workspace = {
  org: Org;
  project: Project;
};

export type RemoteShellTerminalSession = {
  session: RemoteShellSession;
  websocketUrl: string;
  deviceName: string;
  deviceId: string;
};

type Api = ReturnType<typeof createExcaliburApi>;

type ConsoleRuntimeValue = {
  theme: ThemeMode;
  apiBaseUrl: string;
  authMode: "login" | "register";
  email: string;
  password: string;
  displayName: string;
  session: Session | null;
  workspace: Workspace | null;
  orgs: Org[];
  projects: Project[];
  projectFeatures: ProjectFeature[];
  remoteShellSessions: RemoteShellSession[];
  remoteShellEnabled: boolean;
  activeRemoteShell: RemoteShellTerminalSession | null;
  apiKeys: ApiKey[];
  apiKeyError: string | null;
  createdApiKey: ApiKey | null;
  projectData: ProjectData;
  selectedDeviceId: string | undefined;
  selectedDeviceRow: DeviceRow | undefined;
  deviceRows: DeviceRow[];
  filteredDeviceRows: DeviceRow[];
  telemetryValues: number[];
  streamSummaries: StreamSummary[];
  actionSummaries: ActionSummary[];
  allActionSummaries: ActionSummary[];
  alertSummaries: AlertSummary[];
  dashboardStats: DashboardStats;
  metrics: MetricItem[];
  devAuthConfig: DeviceConfig | null;
  search: string;
  busy: boolean;
  error: string | null;
  notice: string | null;
  protocolDeviceName: string | undefined;
  protocolTopics: Array<[string, string]>;
  sidebarUserLabel: string;
  setApiBaseUrl: (value: SetStateAction<string>) => void;
  setAuthMode: (value: SetStateAction<"login" | "register">) => void;
  setEmail: (value: SetStateAction<string>) => void;
  setPassword: (value: SetStateAction<string>) => void;
  setDisplayName: (value: SetStateAction<string>) => void;
  setSearch: (value: SetStateAction<string>) => void;
  setSelectedDeviceId: (value: SetStateAction<string | undefined>) => void;
  getRemoteShellDisabledReason: (deviceId?: string) => string | null;
  handleAuthenticate: (event: FormEvent<HTMLFormElement>) => Promise<void>;
  handleToggleTheme: () => void;
  handleLogout: () => void;
  handleRefresh: () => void;
  handleBootstrapDemo: () => void;
  handleCreateOrg: (name: string) => void;
  handleSelectOrg: (orgId: string) => void;
  handleCreateProject: (name: string) => void;
  handleSelectProject: (projectId: string) => void;
  handleCreateApiKey: (input: ApiKeyCreateInput) => void;
  handleRevokeApiKey: (apiKeyId: string) => void;
  handleCreateDevice: () => void;
  handleDownloadDevAuth: (deviceId?: string) => void;
  handleIngestSample: (deviceId?: string) => void;
  handleCreateDiagnostics: () => void;
  handleCreateOta: () => void;
  handleToggleRemoteShellFeature: (enabled: boolean) => void;
  handleOpenRemoteShell: (deviceId?: string) => void;
  handleCloseRemoteShell: () => void;
  handleDismissRemoteShell: () => void;
  handleCompleteLatest: () => void;
  handleCreateDefaultAlert: () => void;
};

const SESSION_KEY = "excalibur.console.session.v2";
const API_BASE_KEY = "excalibur.console.apiBaseUrl.v1";
const THEME_KEY = "excalibur.console.theme.v1";
const DEFAULT_API_BASE_URL = process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080";
const DEFAULT_SHA256 = "a".repeat(64);
const SESSION_REFRESH_SKEW_MS = 60_000;

const ConsoleRuntimeContext = createContext<ConsoleRuntimeValue | null>(null);

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
    typeof session.expiresAt === "string" &&
    typeof session.refreshExpiresAt === "string" &&
    typeof session.userId === "string"
  );
}

function sessionFromAuth(auth: AuthResponse): Session {
  return {
    expiresAt: auth.expires_at,
    refreshExpiresAt: auth.refresh_expires_at,
    userId: auth.user_id,
  };
}

function expiresBefore(iso: string, cutoffMs: number) {
  const expiresAt = Date.parse(iso);
  return !Number.isFinite(expiresAt) || expiresAt <= cutoffMs;
}

function isActiveRemoteShellSession(session: RemoteShellSession, nowMs = Date.now()) {
  return (
    !session.closed_at &&
    (session.state === "Opening" || session.state === "Active") &&
    Date.parse(session.expires_at) > nowMs
  );
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
    if (existing[0].verified_at) {
      return existing[0];
    }
    return api.finalizeFirmware(existing[0].id, {
      project_id: projectId,
      sha256: existing[0].sha256,
      signature: existing[0].signature ?? null,
      size_bytes: existing[0].size_bytes,
    });
  }
  const created = await ignoreConflict(() =>
    api.createFirmware({
      project_id: projectId,
      component: "main",
      version: "1.0.0",
      object_key: `projects/${projectId}/firmware/main/1.0.0/excalibur-agent.bin`,
      sha256: DEFAULT_SHA256,
      content_type: "application/octet-stream",
      signature: "ed25519:local-dev",
      size_bytes: 1_048_576,
    }),
  );
  if (created) {
    return api.finalizeFirmware(created.id, {
      project_id: projectId,
      sha256: created.sha256,
      signature: created.signature ?? null,
      size_bytes: created.size_bytes,
    });
  }
  const retry = await api.listFirmware(projectId);
  if (!retry[0]) {
    throw new Error("firmware artifact could not be initialized");
  }
  if (retry[0].verified_at) {
    return retry[0];
  }
  return api.finalizeFirmware(retry[0].id, {
    project_id: projectId,
    sha256: retry[0].sha256,
    signature: retry[0].signature ?? null,
    size_bytes: retry[0].size_bytes,
  });
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

export function ConsoleRuntimeProvider({ children }: { children: ReactNode }) {
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
  const [orgs, setOrgs] = useState<Org[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectFeatures, setProjectFeatures] = useState<ProjectFeature[]>([]);
  const [remoteShellSessions, setRemoteShellSessions] = useState<RemoteShellSession[]>([]);
  const [activeRemoteShell, setActiveRemoteShell] = useState<RemoteShellTerminalSession | null>(null);
  const [apiKeys, setApiKeys] = useState<ApiKey[]>([]);
  const [apiKeyError, setApiKeyError] = useState<string | null>(null);
  const [createdApiKey, setCreatedApiKey] = useState<ApiKey | null>(null);
  const [projectData, setProjectData] = useState<ProjectData>(emptyProjectData);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | undefined>();
  const [devAuthConfig, setDevAuthConfig] = useState<DeviceConfig | null>(null);
  const [search, setSearch] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const initializedSessionKey = useRef<string | null>(null);
  const workspaceSessionKey = session && workspace ? `${session.userId}:${session.expiresAt}:${workspace.org.id}:${workspace.project.id}` : null;
  const workspaceSessionKeyRef = useRef<string | null>(null);

  useEffect(() => {
    workspaceSessionKeyRef.current = workspaceSessionKey;
  }, [workspaceSessionKey]);

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
      if (expiresBefore(activeSession.expiresAt, Date.now() + SESSION_REFRESH_SKEW_MS)) {
        if (expiresBefore(activeSession.refreshExpiresAt, Date.now())) {
          clearSession();
          throw new Error("Session expired");
        }
        try {
          const authApi = createExcaliburApi({ baseUrl: apiBaseUrl });
          const auth = await authApi.refreshSession();
          persistSession(sessionFromAuth(auth));
        } catch (refreshError) {
          clearSession();
          throw refreshError;
        }
      }
      return createExcaliburApi({ baseUrl: apiBaseUrl });
    },
    [apiBaseUrl, clearSession, persistSession],
  );

  const loadProjectData = useCallback(async (api: Api, orgId: string, projectId: string) => {
    const [
      devices,
      streams,
      telemetry,
      actions,
      firmware,
      firmwareRollouts,
      alerts,
      audit,
      features,
      shellSessions,
    ] = await Promise.all([
      api.listDevices(projectId),
      api.listStreams(projectId),
      api.queryTelemetry({ projectId, limit: 200 }),
      api.listActions(projectId),
      api.listFirmware(projectId),
      api.listFirmwareRollouts(projectId),
      api.listAlerts(projectId),
      api.listAudit(orgId, projectId),
      api.listProjectFeatures(projectId),
      api.listRemoteShellSessions(projectId),
    ]);

    setProjectData({ devices, streams, telemetry, actions, firmware, firmwareRollouts, alerts, audit });
    setProjectFeatures(features);
    setRemoteShellSessions(shellSessions);
    setSelectedDeviceId((current) =>
      current && devices.some((device) => device.id === current) ? current : devices[0]?.id,
    );
  }, []);

  const refreshWorkspaceManagement = useCallback(async (api: Api, activeWorkspace: Workspace) => {
    const [nextOrgs, nextProjects] = await Promise.all([
      api.listOrgs(),
      api.listProjects(activeWorkspace.org.id),
    ]);
    setOrgs(nextOrgs);
    setProjects(nextProjects);

    try {
      const nextApiKeys = await api.listApiKeys(activeWorkspace.org.id, activeWorkspace.project.id);
      setApiKeys(nextApiKeys);
      setApiKeyError(null);
    } catch (apiKeyListError) {
      setApiKeys([]);
      setApiKeyError(formatError(apiKeyListError));
    }
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
        const nextWorkspace = { org, project };
        setWorkspace(nextWorkspace);
        await Promise.all([
          refreshWorkspaceManagement(api, nextWorkspace),
          loadProjectData(api, org.id, project.id),
        ]);
        setNotice("Workspace ready");
        return true;
      } catch (loadError) {
        setError(formatError(loadError));
        return false;
      } finally {
        setBusy(false);
      }
    },
    [getApiForSession, loadProjectData, refreshWorkspaceManagement],
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
      const sessionKey = `${apiBaseUrl}:${session.userId}:${session.expiresAt}`;
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
      setOrgs([]);
      setProjects([]);
      setProjectFeatures([]);
      setRemoteShellSessions([]);
      setActiveRemoteShell(null);
      setApiKeys([]);
      setApiKeyError(null);
      setCreatedApiKey(null);
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
      setCreatedApiKey(null);
      try {
        const api = await getApiForSession(session);
        await work(api, workspace);
        await Promise.all([
          loadProjectData(api, workspace.org.id, workspace.project.id),
          refreshWorkspaceManagement(api, workspace),
        ]);
        setNotice(success);
      } catch (mutationError) {
        setError(formatError(mutationError));
      } finally {
        setBusy(false);
      }
    },
    [getApiForSession, loadProjectData, refreshWorkspaceManagement, session, workspace],
  );

  const activateWorkspace = useCallback(
    async (api: Api, nextWorkspace: Workspace, success: string) => {
      await ensureDefaultControlPlane(api, nextWorkspace.project.id);
      setWorkspace(nextWorkspace);
      setCreatedApiKey(null);
      setDevAuthConfig(null);
      setActiveRemoteShell(null);
      await Promise.all([
        refreshWorkspaceManagement(api, nextWorkspace),
        loadProjectData(api, nextWorkspace.org.id, nextWorkspace.project.id),
      ]);
      setNotice(success);
    },
    [loadProjectData, refreshWorkspaceManagement],
  );

  const selectedDevice = useMemo(
    () => projectData.devices.find((device) => device.id === selectedDeviceId),
    [projectData.devices, selectedDeviceId],
  );
  const remoteShellEnabled = useMemo(
    () => projectFeatures.some((feature) => feature.feature === "remote_shell" && feature.enabled),
    [projectFeatures],
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
    () => toActionSummaries(projectData.actions, projectData.devices, 6),
    [projectData.actions, projectData.devices],
  );
  const allActionSummaries = useMemo(
    () => toActionSummaries(projectData.actions, projectData.devices),
    [projectData.actions, projectData.devices],
  );
  const alertSummaries = useMemo(
    () => toAlertSummaries(projectData.alerts, projectData.devices),
    [projectData.alerts, projectData.devices],
  );
  const dashboardStats = useMemo(
    () => toDashboardStats(projectData, deviceRows, alertSummaries),
    [alertSummaries, deviceRows, projectData],
  );
  const metrics = useMemo<MetricItem[]>(
    () => [
      {
        label: "Connected devices",
        value: `${dashboardStats.onlineDevices}/${dashboardStats.totalDevices}`,
        delta:
          dashboardStats.totalDevices === 0 ? "no devices" : `${dashboardStats.onlinePercent}% online`,
        tone: "teal",
        icon: Wifi,
      },
      {
        label: "Telemetry rows",
        value: formatCount(dashboardStats.telemetryRows),
        delta: `${streamSummaries.length} streams`,
        tone: "signal",
        icon: RadioTower,
      },
      {
        label: "Open actions",
        value: String(dashboardStats.openActions),
        delta: `${dashboardStats.totalActions} total`,
        tone: "amber",
        icon: Zap,
      },
      {
        label: "Alert pressure",
        value: String(dashboardStats.firingAlerts),
        delta: `${dashboardStats.totalAlerts} rules`,
        tone: dashboardStats.firingAlerts > 0 ? "danger" : "teal",
        icon: Bell,
      },
    ],
    [dashboardStats, streamSummaries.length],
  );

  const getRemoteShellDisabledReason = useCallback(
    (deviceId?: string) => {
      const targetDeviceId = deviceId ?? selectedDeviceId;
      if (!targetDeviceId) {
        return "No device selected";
      }
      if (!remoteShellEnabled) {
        return "Remote shell beta is off";
      }
      if (busy) {
        return "Console is busy";
      }
      if (
        remoteShellSessions.some(
          (session) => session.device_id === targetDeviceId && isActiveRemoteShellSession(session),
        )
      ) {
        return "Active session already exists";
      }
      return null;
    },
    [busy, remoteShellEnabled, remoteShellSessions, selectedDeviceId],
  );

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

  const handleCreateOrg = useCallback(
    (name: string) => {
      if (!session) {
        return;
      }
      setBusy(true);
      setError(null);
      setCreatedApiKey(null);
      void (async () => {
        try {
          const api = await getApiForSession(session);
          const org = await api.createOrg({
            name,
            slug: slugifyWorkspaceName(name, "org"),
          });
          const project = await ensureProject(api, org);
          await activateWorkspace(api, { org, project }, `Organization ${org.name} created`);
        } catch (orgError) {
          setError(formatError(orgError));
        } finally {
          setBusy(false);
        }
      })();
    },
    [activateWorkspace, getApiForSession, session],
  );

  const handleSelectOrg = useCallback(
    (orgId: string) => {
      if (!session || workspace?.org.id === orgId) {
        return;
      }
      const org = orgs.find((candidate) => candidate.id === orgId);
      if (!org) {
        return;
      }
      setBusy(true);
      setError(null);
      setCreatedApiKey(null);
      void (async () => {
        try {
          const api = await getApiForSession(session);
          const project = await ensureProject(api, org);
          await activateWorkspace(api, { org, project }, `Switched to ${org.name}`);
        } catch (orgError) {
          setError(formatError(orgError));
        } finally {
          setBusy(false);
        }
      })();
    },
    [activateWorkspace, getApiForSession, orgs, session, workspace?.org.id],
  );

  const handleCreateProject = useCallback(
    (name: string) => {
      if (!session || !workspace) {
        return;
      }
      setBusy(true);
      setError(null);
      setCreatedApiKey(null);
      void (async () => {
        try {
          const api = await getApiForSession(session);
          const project = await api.createProject({
            org_id: workspace.org.id,
            name,
            slug: slugifyWorkspaceName(name, "project"),
          });
          await activateWorkspace(api, { org: workspace.org, project }, `Project ${project.name} created`);
        } catch (projectError) {
          setError(formatError(projectError));
        } finally {
          setBusy(false);
        }
      })();
    },
    [activateWorkspace, getApiForSession, session, workspace],
  );

  const handleSelectProject = useCallback(
    (projectId: string) => {
      if (!session || !workspace || workspace.project.id === projectId) {
        return;
      }
      const project = projects.find((candidate) => candidate.id === projectId);
      if (!project) {
        return;
      }
      setBusy(true);
      setError(null);
      void (async () => {
        try {
          const api = await getApiForSession(session);
          await activateWorkspace(api, { org: workspace.org, project }, `Switched to ${project.name}`);
        } catch (projectError) {
          setError(formatError(projectError));
        } finally {
          setBusy(false);
        }
      })();
    },
    [activateWorkspace, getApiForSession, projects, session, workspace],
  );

  const handleCreateApiKey = useCallback(
    (input: ApiKeyCreateInput) => {
      if (!session || !workspace) {
        return;
      }
      setBusy(true);
      setError(null);
      setCreatedApiKey(null);
      const mutationWorkspaceKey = workspaceSessionKeyRef.current;
      void (async () => {
        try {
          const api = await getApiForSession(session);
          const preset = getApiKeyScopePreset(input.presetId);
          const expiresAt =
            input.expiresInDays === null
              ? null
              : new Date(Date.now() + input.expiresInDays * 24 * 60 * 60 * 1000).toISOString();
          const created = await api.createApiKey({
            org_id: workspace.org.id,
            project_id: workspace.project.id,
            name: input.name,
            scopes: [...preset.scopes],
            expires_at: expiresAt,
          });
          if (workspaceSessionKeyRef.current !== mutationWorkspaceKey) {
            return;
          }
          await Promise.all([
            refreshWorkspaceManagement(api, workspace),
            loadProjectData(api, workspace.org.id, workspace.project.id),
          ]);
          if (workspaceSessionKeyRef.current !== mutationWorkspaceKey) {
            return;
          }
          setCreatedApiKey(created);
          setNotice("API key created");
        } catch (apiKeyErrorValue) {
          if (workspaceSessionKeyRef.current === mutationWorkspaceKey) {
            setCreatedApiKey(null);
            setError(formatError(apiKeyErrorValue));
          }
        } finally {
          setBusy(false);
        }
      })();
    },
    [getApiForSession, loadProjectData, refreshWorkspaceManagement, session, workspace],
  );

  const handleRevokeApiKey = useCallback(
    (apiKeyId: string) => {
      if (!session || !workspace) {
        return;
      }
      const apiKey = apiKeys.find((candidate) => candidate.id === apiKeyId);
      const label = apiKey?.name ?? apiKeyId;
      if (!window.confirm(`Revoke API key "${label}" for ${workspace.project.name}?`)) {
        return;
      }
      setBusy(true);
      setError(null);
      void (async () => {
        try {
          const api = await getApiForSession(session);
          await api.revokeApiKey(apiKeyId, workspace.org.id);
          setCreatedApiKey(null);
          await Promise.all([
            refreshWorkspaceManagement(api, workspace),
            loadProjectData(api, workspace.org.id, workspace.project.id),
          ]);
          setNotice("API key revoked");
        } catch (apiKeyErrorValue) {
          setError(formatError(apiKeyErrorValue));
        } finally {
          setBusy(false);
        }
      })();
    },
    [apiKeys, getApiForSession, loadProjectData, refreshWorkspaceManagement, session, workspace],
  );

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
      const diagnostics = await api.createDiagnosticsSession({
        project_id: activeWorkspace.project.id,
        device_id: selectedDeviceId,
        paths: ["/var/log/excalibur-agent"],
        include_logs: true,
        include_system_stats: true,
      });
      await api.updateActionStatus(diagnostics.action.id, {
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
      const rollout = await api.createFirmwareRollout(firmware.id, {
        project_id: activeWorkspace.project.id,
        device_ids: [selectedDeviceId],
        rollback_strategy: "previous_version",
      });
      await api.updateActionStatus(rollout.action_id, {
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

  const handleToggleRemoteShellFeature = useCallback(
    (enabled: boolean) => {
      void runProjectMutation(`Remote shell ${enabled ? "enabled" : "disabled"}`, async (api, activeWorkspace) => {
        const feature = await api.setRemoteShellFeature(activeWorkspace.project.id, enabled);
        setProjectFeatures((current) => [
          feature,
          ...current.filter((candidate) => candidate.feature !== feature.feature),
        ]);
      });
    },
    [runProjectMutation],
  );

  const handleOpenRemoteShell = useCallback(
    (deviceId?: string) => {
      if (!session || !workspace) {
        return;
      }
      const targetDeviceId = deviceId ?? selectedDeviceId;
      const disabledReason = getRemoteShellDisabledReason(targetDeviceId);
      if (disabledReason) {
        setError(disabledReason);
        return;
      }
      const targetDevice = projectData.devices.find((device) => device.id === targetDeviceId);
      if (!targetDevice) {
        setError("Device not found");
        return;
      }
      if (
        !window.confirm(
          `Open a 10 minute remote shell for ${targetDevice.name} (${targetDevice.id})?`,
        )
      ) {
        return;
      }
      setBusy(true);
      setError(null);
      setNotice(null);
      void (async () => {
        try {
          const api = await getApiForSession(session);
          const created = await api.createRemoteShellSession({
            project_id: workspace.project.id,
            device_id: targetDevice.id,
            ttl_seconds: 600,
          });
          setRemoteShellSessions((current) => [
            created.session,
            ...current.filter((candidate) => candidate.id !== created.session.id),
          ]);
          setProjectData((current) => ({
            ...current,
            actions: [
              created.action,
              ...current.actions.filter((candidate) => candidate.id !== created.action.id),
            ],
          }));
          setActiveRemoteShell({
            session: created.session,
            websocketUrl: created.operator_websocket_url,
            deviceName: targetDevice.name,
            deviceId: targetDevice.id,
          });
          await loadProjectData(api, workspace.org.id, workspace.project.id);
          setNotice("Remote shell session opened");
        } catch (remoteShellError) {
          setError(formatError(remoteShellError));
        } finally {
          setBusy(false);
        }
      })();
    },
    [
      getApiForSession,
      getRemoteShellDisabledReason,
      loadProjectData,
      projectData.devices,
      selectedDeviceId,
      session,
      workspace,
    ],
  );

  const handleCloseRemoteShell = useCallback(() => {
    if (!session || !workspace || !activeRemoteShell) {
      return;
    }
    setBusy(true);
    setError(null);
    void (async () => {
      try {
        const api = await getApiForSession(session);
        const closed = await api.closeRemoteShellSession(activeRemoteShell.session.id);
        setRemoteShellSessions((current) => [
          closed,
          ...current.filter((candidate) => candidate.id !== closed.id),
        ]);
        setActiveRemoteShell((current) =>
          current && current.session.id === closed.id ? { ...current, session: closed } : current,
        );
        await loadProjectData(api, workspace.org.id, workspace.project.id);
        setNotice("Remote shell session closed");
      } catch (remoteShellError) {
        setError(formatError(remoteShellError));
      } finally {
        setBusy(false);
      }
    })();
  }, [activeRemoteShell, getApiForSession, loadProjectData, session, workspace]);

  const handleDismissRemoteShell = useCallback(() => {
    setActiveRemoteShell(null);
  }, []);

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
      const diagnostics = await api.createDiagnosticsSession({
        project_id: activeWorkspace.project.id,
        device_id: device.id,
        paths: ["/var/log/excalibur-agent"],
        include_logs: true,
        include_system_stats: true,
      });
      await api.updateActionStatus(diagnostics.action.id, {
        project_id: activeWorkspace.project.id,
        device_id: device.id,
        state: "Completed",
        progress: 100,
        errors: [],
      });
    });
  }, [runProjectMutation]);

  const protocolDevice = selectedDevice ?? projectData.devices[0];
  const protocolTopics = useMemo<Array<[string, string]>>(() => {
    if (!protocolDevice || !workspace) {
      return [];
    }
    return [
      ["telemetry publish", telemetryTopic(workspace.project.id, protocolDevice.id, SYSTEM_STREAM)],
      ["shadow publish", shadowTopic(workspace.project.id, protocolDevice.id)],
      ["commands subscribe", commandTopic(workspace.project.id, protocolDevice.id)],
      ["command status", commandStatusTopic(workspace.project.id, protocolDevice.id)],
    ];
  }, [protocolDevice, workspace]);
  const sidebarUserLabel = session?.userId.slice(0, 8) ?? "User";

  const value = useMemo<ConsoleRuntimeValue>(
    () => ({
      theme,
      apiBaseUrl,
      authMode,
      email,
      password,
      displayName,
      session,
      workspace,
      orgs,
      projects,
      projectFeatures,
      remoteShellSessions,
      remoteShellEnabled,
      activeRemoteShell,
      apiKeys,
      apiKeyError,
      createdApiKey,
      projectData,
      selectedDeviceId,
      selectedDeviceRow,
      deviceRows,
      filteredDeviceRows,
      telemetryValues,
      streamSummaries,
      actionSummaries,
      allActionSummaries,
      alertSummaries,
      dashboardStats,
      metrics,
      devAuthConfig,
      search,
      busy,
      error,
      notice,
      protocolDeviceName: protocolDevice?.name,
      protocolTopics,
      sidebarUserLabel,
      setApiBaseUrl,
      setAuthMode,
      setEmail,
      setPassword,
      setDisplayName,
      setSearch,
      setSelectedDeviceId,
      getRemoteShellDisabledReason,
      handleAuthenticate,
      handleToggleTheme,
      handleLogout,
      handleRefresh,
      handleBootstrapDemo,
      handleCreateOrg,
      handleSelectOrg,
      handleCreateProject,
      handleSelectProject,
      handleCreateApiKey,
      handleRevokeApiKey,
      handleCreateDevice,
      handleDownloadDevAuth,
      handleIngestSample,
      handleCreateDiagnostics,
      handleCreateOta,
      handleToggleRemoteShellFeature,
      handleOpenRemoteShell,
      handleCloseRemoteShell,
      handleDismissRemoteShell,
      handleCompleteLatest,
      handleCreateDefaultAlert,
    }),
    [
      actionSummaries,
      allActionSummaries,
      alertSummaries,
      activeRemoteShell,
      apiBaseUrl,
      apiKeyError,
      apiKeys,
      authMode,
      busy,
      createdApiKey,
      dashboardStats,
      devAuthConfig,
      deviceRows,
      displayName,
      email,
      error,
      filteredDeviceRows,
      getRemoteShellDisabledReason,
      handleBootstrapDemo,
      handleCompleteLatest,
      handleCreateApiKey,
      handleCreateDefaultAlert,
      handleCreateDevice,
      handleCreateDiagnostics,
      handleCreateOta,
      handleCreateOrg,
      handleDownloadDevAuth,
      handleCloseRemoteShell,
      handleDismissRemoteShell,
      handleOpenRemoteShell,
      handleToggleRemoteShellFeature,
      handleIngestSample,
      handleLogout,
      handleRefresh,
      handleRevokeApiKey,
      handleSelectOrg,
      handleSelectProject,
      handleToggleTheme,
      metrics,
      notice,
      orgs,
      password,
      projectFeatures,
      projectData,
      projects,
      protocolDevice?.name,
      protocolTopics,
      search,
      selectedDeviceId,
      selectedDeviceRow,
      session,
      sidebarUserLabel,
      remoteShellEnabled,
      remoteShellSessions,
      streamSummaries,
      telemetryValues,
      theme,
      workspace,
    ],
  );

  return <ConsoleRuntimeContext.Provider value={value}>{children}</ConsoleRuntimeContext.Provider>;
}

export function useConsoleRuntime() {
  const runtime = useContext(ConsoleRuntimeContext);
  if (!runtime) {
    throw new Error("useConsoleRuntime must be used within ConsoleRuntimeProvider");
  }
  return runtime;
}
