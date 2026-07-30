import type {
  ActionResponse,
  AlertKindDto,
  AlertEventResponse,
  AlertRuleResponse,
  ApiKeyResponse,
  AuditLogResponse,
  AuthResponse,
  CreateApiKeyRequest,
  ActionStatusRequest,
  ActionTransitionRequest,
  CreateActionRequest,
  CreateDiagnosticsSessionRequest,
  CreateFirmwareRequest,
  DashboardResponse,
  DeviceConfigResponse,
  DeviceResponse,
  DiagnosticsFinalizeRequest,
  DiagnosticsSessionCreateResponse,
  DiagnosticsSessionResponse,
  FirmwareArtifactResponse,
  FirmwareFinalizeRequest,
  FirmwareRolloutRequest,
  FirmwareRolloutResponse,
  JsonValue,
  LoginRequest,
  LogoutResponse,
  OrgResponse,
  ProjectResponse,
  RefreshRequest,
  RegisterRequest,
  SignedObjectUrl,
  StreamDefinitionResponse,
  StreamFieldDto,
  StreamFieldTypeDto,
  TelemetryAggregateBucketResponse,
  TelemetryPointResponse,
} from "./generated/api-types";

export type {
  AuthResponse,
  CreateApiKeyRequest,
  ActionStatusRequest,
  ActionTransitionRequest,
  CreateActionRequest,
  CreateDiagnosticsSessionRequest,
  CreateFirmwareRequest,
  DiagnosticsFinalizeRequest,
  FirmwareFinalizeRequest,
  FirmwareRolloutRequest,
  JsonPrimitive,
  JsonValue,
  LoginRequest,
  LogoutResponse,
  RefreshRequest,
  RegisterRequest,
  SignedObjectUrl,
} from "./generated/api-types";

export type Action = ActionResponse;
export type AlertEvent = AlertEventResponse;
export type AlertKind = AlertKindDto;
export type AlertRule = AlertRuleResponse;
export type ApiKey = ApiKeyResponse;
export type AuditLog = AuditLogResponse;
export type Dashboard = DashboardResponse;
export type Device = DeviceResponse;
export type DeviceConfig = DeviceConfigResponse;
export type DiagnosticsSession = DiagnosticsSessionResponse;
export type DiagnosticsSessionCreate = DiagnosticsSessionCreateResponse;
export type FirmwareArtifact = FirmwareArtifactResponse;
export type FirmwareRollout = FirmwareRolloutResponse;
export type Org = OrgResponse;
export type Project = ProjectResponse;
export type StreamDefinition = StreamDefinitionResponse;
export type StreamField = StreamFieldDto;
export type StreamFieldType = StreamFieldTypeDto;
export type TelemetryAggregateBucket = TelemetryAggregateBucketResponse;
export type TelemetryPoint = TelemetryPointResponse;

export type ApiClientOptions = {
  baseUrl?: string;
  token?: string | null;
  credentials?: RequestCredentials;
  fetcher?: typeof fetch;
};

export class ExcaliburApiError extends Error {
  readonly status: number;
  readonly body: unknown;

  constructor(status: number, message: string, body: unknown) {
    super(message);
    this.name = "ExcaliburApiError";
    this.status = status;
    this.body = body;
  }
}

const DEFAULT_API_BASE_URL = process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080";

export function normalizeApiBaseUrl(baseUrl = DEFAULT_API_BASE_URL) {
  return baseUrl.replace(/\/+$/, "");
}

export function buildApiUrl(
  baseUrl: string,
  path: string,
  query?: Record<string, string | number | boolean | null | undefined>,
) {
  const url = new URL(path, `${normalizeApiBaseUrl(baseUrl)}/`);
  Object.entries(query ?? {}).forEach(([key, value]) => {
    if (value !== undefined && value !== null && value !== "") {
      url.searchParams.set(key, String(value));
    }
  });
  return url.toString();
}

export function createExcaliburApi(options: ApiClientOptions = {}) {
  const baseUrl = normalizeApiBaseUrl(options.baseUrl);
  const fetcher = options.fetcher ?? fetch;
  const token = options.token;
  const credentials = options.credentials ?? "include";

  async function request<T>(
    path: string,
    init: RequestInit & {
      query?: Record<string, string | number | boolean | null | undefined>;
      bodyJson?: unknown;
    } = {},
  ): Promise<T> {
    const headers = new Headers(init.headers);
    if (token) {
      headers.set("authorization", `Bearer ${token}`);
    }
    if (init.bodyJson !== undefined) {
      headers.set("content-type", "application/json");
    }

    const response = await fetcher(buildApiUrl(baseUrl, path, init.query), {
      ...init,
      credentials: init.credentials ?? credentials,
      headers,
      body: init.bodyJson === undefined ? init.body : JSON.stringify(init.bodyJson),
    });
    const text = await response.text();
    let body: unknown = null;
    if (text.trim()) {
      try {
        body = JSON.parse(text);
      } catch {
        const message = response.ok
          ? "API response was not valid JSON"
          : `API request failed with ${response.status}`;
        throw new ExcaliburApiError(response.status, message, text);
      }
    }

    if (!response.ok) {
      const message =
        body && typeof body === "object" && "error" in body
          ? String((body as { error: unknown }).error)
          : `API request failed with ${response.status}`;
      throw new ExcaliburApiError(response.status, message, body);
    }

    return body as T;
  }

  return {
    baseUrl,
    register: (body: RegisterRequest) =>
      request<AuthResponse>("/api/v1/auth/register", { method: "POST", bodyJson: body }),
    login: (body: LoginRequest) =>
      request<AuthResponse>("/api/v1/auth/login", { method: "POST", bodyJson: body }),
    refreshSession: (body?: RefreshRequest) =>
      request<AuthResponse>("/api/v1/auth/refresh", {
        method: "POST",
        ...(body ? { bodyJson: body } : {}),
      }),
    logout: () => request<LogoutResponse>("/api/v1/auth/logout", { method: "POST" }),
    listApiKeys: (orgId: string, projectId?: string) =>
      request<ApiKey[]>("/api/v1/api-keys", {
        query: { org_id: orgId, project_id: projectId },
      }),
    createApiKey: (body: CreateApiKeyRequest) =>
      request<ApiKey>("/api/v1/api-keys", { method: "POST", bodyJson: body }),
    revokeApiKey: (apiKeyId: string, orgId: string) =>
      request<ApiKey>(`/api/v1/api-keys/${apiKeyId}/revoke`, {
        method: "POST",
        query: { org_id: orgId },
      }),
    listOrgs: () => request<Org[]>("/api/v1/orgs"),
    createOrg: (body: { name: string; slug: string }) =>
      request<Org>("/api/v1/orgs", { method: "POST", bodyJson: body }),
    listProjects: (orgId: string) =>
      request<Project[]>("/api/v1/projects", { query: { org_id: orgId } }),
    createProject: (body: { org_id: string; name: string; slug: string }) =>
      request<Project>("/api/v1/projects", { method: "POST", bodyJson: body }),
    listDevices: (projectId: string) =>
      request<Device[]>("/api/v1/devices", { query: { project_id: projectId } }),
    createDevice: (body: { project_id: string; name: string; metadata: JsonValue }) =>
      request<Device>("/api/v1/devices", { method: "POST", bodyJson: body }),
    provisionDevAuth: (deviceId: string, projectId: string) =>
      request<DeviceConfig>(`/api/v1/devices/${deviceId}/provision/dev-auth`, {
        method: "POST",
        bodyJson: { project_id: projectId },
      }),
    listStreams: (projectId: string) =>
      request<StreamDefinition[]>("/api/v1/streams", { query: { project_id: projectId } }),
    createStream: (body: { project_id: string; name: string; fields: StreamField[] }) =>
      request<StreamDefinition>("/api/v1/streams", { method: "POST", bodyJson: body }),
    ingestTelemetry: (body: { topic: string; payload: JsonValue }) =>
      request<{ written?: number; shadow?: string; status?: string }>("/api/v1/telemetry", {
        method: "POST",
        bodyJson: body,
      }),
    queryTelemetry: (query: {
      projectId: string;
      deviceId?: string;
      stream?: string;
      limit?: number;
    }) =>
      request<TelemetryPoint[]>("/api/v1/telemetry", {
        query: {
          project_id: query.projectId,
          device_id: query.deviceId,
          stream: query.stream,
          limit: query.limit,
        },
      }),
    aggregateTelemetry: (query: {
      projectId: string;
      deviceId?: string;
      stream: string;
      field?: string;
      from?: string;
      to?: string;
      bucketSeconds?: number;
      limit?: number;
    }) =>
      request<TelemetryAggregateBucket[]>("/api/v1/telemetry/aggregate", {
        query: {
          project_id: query.projectId,
          device_id: query.deviceId,
          stream: query.stream,
          field: query.field,
          from: query.from,
          to: query.to,
          bucket_seconds: query.bucketSeconds,
          limit: query.limit,
        },
      }),
    listActions: (projectId: string) =>
      request<Action[]>("/api/v1/actions", { query: { project_id: projectId } }),
    createAction: (body: CreateActionRequest) =>
      request<Action>("/api/v1/actions", { method: "POST", bodyJson: body }),
    approveAction: (actionId: string, body: ActionTransitionRequest) =>
      request<Action>(`/api/v1/actions/${actionId}/approve`, {
        method: "POST",
        bodyJson: body,
      }),
    retryAction: (actionId: string, body: ActionTransitionRequest) =>
      request<Action>(`/api/v1/actions/${actionId}/retry`, {
        method: "POST",
        bodyJson: body,
      }),
    cancelAction: (actionId: string, body: ActionTransitionRequest) =>
      request<Action>(`/api/v1/actions/${actionId}/cancel`, {
        method: "POST",
        bodyJson: body,
      }),
    updateActionStatus: (actionId: string, body: ActionStatusRequest) =>
      request<Action>(`/api/v1/actions/${actionId}/status`, {
        method: "POST",
        bodyJson: body,
      }),
    listFirmware: (projectId: string) =>
      request<FirmwareArtifact[]>("/api/v1/firmware", { query: { project_id: projectId } }),
    createFirmware: (body: CreateFirmwareRequest) =>
      request<FirmwareArtifact>("/api/v1/firmware", { method: "POST", bodyJson: body }),
    createFirmwareUploadUrl: (firmwareId: string, projectId: string) =>
      request<SignedObjectUrl>(`/api/v1/firmware/${firmwareId}/upload-url`, {
        method: "POST",
        query: { project_id: projectId },
      }),
    createFirmwareDownloadUrl: (firmwareId: string, projectId: string) =>
      request<SignedObjectUrl>(`/api/v1/firmware/${firmwareId}/download-url`, {
        method: "POST",
        query: { project_id: projectId },
      }),
    finalizeFirmware: (firmwareId: string, body: FirmwareFinalizeRequest) =>
      request<FirmwareArtifact>(`/api/v1/firmware/${firmwareId}/finalize`, {
        method: "POST",
        bodyJson: body,
      }),
    createFirmwareRollout: (firmwareId: string, body: FirmwareRolloutRequest) =>
      request<FirmwareRollout>(`/api/v1/firmware/${firmwareId}/rollout`, {
        method: "POST",
        bodyJson: body,
      }),
    listFirmwareRollouts: (projectId: string) =>
      request<FirmwareRollout[]>("/api/v1/firmware-rollouts", {
        query: { project_id: projectId },
      }),
    listDashboards: (projectId: string) =>
      request<Dashboard[]>("/api/v1/dashboards", { query: { project_id: projectId } }),
    createDashboard: (body: { project_id: string; name: string; layout: JsonValue }) =>
      request<Dashboard>("/api/v1/dashboards", { method: "POST", bodyJson: body }),
    listAlerts: (projectId: string) =>
      request<AlertRule[]>("/api/v1/alerts", { query: { project_id: projectId } }),
    createAlert: (body: { project_id: string; name: string; kind: AlertKind; expression: JsonValue }) =>
      request<AlertRule>("/api/v1/alerts", { method: "POST", bodyJson: body }),
    listAlertEvents: (projectId: string, state?: "Firing" | "Resolved") =>
      request<AlertEvent[]>("/api/v1/alert-events", {
        query: { project_id: projectId, state },
      }),
    createDiagnosticsSession: (body: CreateDiagnosticsSessionRequest) =>
      request<DiagnosticsSessionCreate>("/api/v1/diagnostics/sessions", {
        method: "POST",
        bodyJson: body,
      }),
    listDiagnosticsSessions: (projectId: string) =>
      request<DiagnosticsSession[]>("/api/v1/diagnostics/sessions", {
        query: { project_id: projectId },
      }),
    finalizeDiagnosticsSession: (sessionId: string, body: DiagnosticsFinalizeRequest) =>
      request<DiagnosticsSession>(`/api/v1/diagnostics/sessions/${sessionId}/finalize`, {
        method: "POST",
        bodyJson: body,
      }),
    createDiagnosticsDownloadUrl: (sessionId: string, projectId: string) =>
      request<SignedObjectUrl>(`/api/v1/diagnostics/sessions/${sessionId}/download-url`, {
        method: "POST",
        query: { project_id: projectId },
      }),
    listAudit: (orgId: string, projectId?: string) =>
      request<AuditLog[]>("/api/v1/audit", {
        query: { org_id: orgId, project_id: projectId },
      }),
  };
}
