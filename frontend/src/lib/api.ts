export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

export type AuthResponse = {
  token: string;
  refresh_token: string;
  expires_at: string;
  refresh_expires_at: string;
  user_id: string;
};

export type LogoutResponse = {
  status: string;
};

export type Org = {
  id: string;
  name: string;
  slug: string;
  created_at: string;
};

export type Project = {
  id: string;
  org_id: string;
  name: string;
  slug: string;
  created_at: string;
};

export type Device = {
  id: string;
  project_id: string;
  name: string;
  status: string;
  metadata: JsonValue;
  last_seen_at: string | null;
  latest_shadow: JsonValue;
  created_at: string;
};

export type DeviceConfig = {
  broker: string;
  port: number;
  project_id: string;
  device_id: string;
  authentication: {
    ca_certificate: string;
    device_certificate: string;
    device_private_key?: string;
    device_private_key_path?: string;
  };
  provisioning_mode: "Csr" | "DevGeneratedKeypair";
  production: boolean;
};

export type StreamFieldType = "String" | "Integer" | "Float" | "Boolean" | "Json";

export type StreamField = {
  name: string;
  field_type: StreamFieldType;
  required: boolean;
};

export type StreamDefinition = {
  id: string;
  project_id: string;
  name: string;
  fields: StreamField[];
  created_at: string;
};

export type TelemetryPoint = {
  project_id: string;
  device_id: string;
  stream: string;
  sequence: number;
  ts: string;
  payload: JsonValue;
  ingested_at: string;
};

export type Action = {
  id: string;
  project_id: string;
  device_ids: string[];
  name: string;
  payload: JsonValue;
  state: string;
  progress: number;
  errors: string[];
  created_by: string | null;
  created_at: string;
  updated_at: string;
};

export type FirmwareArtifact = {
  id: string;
  project_id: string;
  component: string;
  version: string;
  object_key: string;
  sha256: string;
  size_bytes: number;
  active: boolean;
  created_at: string;
};

export type AlertKind = "Offline" | "Threshold" | "WindowAggregation";

export type AlertRule = {
  id: string;
  project_id: string;
  name: string;
  kind: string;
  expression: JsonValue;
  enabled: boolean;
};

export type Dashboard = {
  id: string;
  project_id: string;
  name: string;
  layout: JsonValue;
};

export type AuditLog = {
  id: string;
  org_id: string;
  project_id: string | null;
  actor_id: string | null;
  action: string;
  resource: string;
  metadata: JsonValue;
  created_at: string;
};

export type ApiKey = {
  id: string;
  org_id: string;
  project_id: string | null;
  name: string;
  scopes: string[];
  expires_at: string | null;
  revoked_at: string | null;
  last_used_at: string | null;
  created_by: string | null;
  created_at: string;
  key?: string;
};

export type ApiClientOptions = {
  baseUrl?: string;
  token?: string | null;
  credentials?: RequestCredentials;
  fetcher?: typeof fetch;
};

export type RegisterRequest = {
  email: string;
  password: string;
  display_name: string;
};

export type LoginRequest = {
  email: string;
  password: string;
};

export type RefreshRequest = {
  refresh_token: string;
};

export type CreateApiKeyRequest = {
  org_id: string;
  project_id?: string | null;
  name: string;
  scopes: string[];
  expires_at?: string | null;
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
    listActions: (projectId: string) =>
      request<Action[]>("/api/v1/actions", { query: { project_id: projectId } }),
    createAction: (body: {
      project_id: string;
      device_ids: string[];
      name: string;
      payload: JsonValue;
    }) => request<Action>("/api/v1/actions", { method: "POST", bodyJson: body }),
    updateActionStatus: (
      actionId: string,
      body: {
        project_id: string;
        device_id: string;
        state: "Queued" | "WaitingApproval" | "Running" | "Completed" | "Failed" | "Cancelled" | "TimedOut";
        progress: number;
        errors: string[];
      },
    ) =>
      request<Action>(`/api/v1/actions/${actionId}/status`, {
        method: "POST",
        bodyJson: body,
      }),
    listFirmware: (projectId: string) =>
      request<FirmwareArtifact[]>("/api/v1/firmware", { query: { project_id: projectId } }),
    createFirmware: (body: {
      project_id: string;
      component: string;
      version: string;
      object_key: string;
      sha256: string;
      size_bytes: number;
    }) => request<FirmwareArtifact>("/api/v1/firmware", { method: "POST", bodyJson: body }),
    listDashboards: (projectId: string) =>
      request<Dashboard[]>("/api/v1/dashboards", { query: { project_id: projectId } }),
    createDashboard: (body: { project_id: string; name: string; layout: JsonValue }) =>
      request<Dashboard>("/api/v1/dashboards", { method: "POST", bodyJson: body }),
    listAlerts: (projectId: string) =>
      request<AlertRule[]>("/api/v1/alerts", { query: { project_id: projectId } }),
    createAlert: (body: { project_id: string; name: string; kind: AlertKind; expression: JsonValue }) =>
      request<AlertRule>("/api/v1/alerts", { method: "POST", bodyJson: body }),
    listAudit: (orgId: string, projectId?: string) =>
      request<AuditLog[]>("/api/v1/audit", {
        query: { org_id: orgId, project_id: projectId },
      }),
  };
}
