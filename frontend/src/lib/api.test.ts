import { describe, expect, it, vi } from "vitest";
import { ExcaliburApiError, buildApiUrl, createExcaliburApi, normalizeApiBaseUrl } from "./api";

describe("Excalibur API client", () => {
  it("normalizes base URLs and query parameters", () => {
    expect(normalizeApiBaseUrl("http://localhost:8080///")).toBe("http://localhost:8080");
    expect(
      buildApiUrl("http://localhost:8080/", "/api/v1/devices", {
        project_id: "project-1",
        empty: "",
        limit: 100,
      }),
    ).toBe("http://localhost:8080/api/v1/devices?project_id=project-1&limit=100");
  });

  it("sends bearer auth and JSON bodies", async () => {
    const fetcher = vi.fn(async () => new Response(JSON.stringify({ id: "org-1" })));
    const api = createExcaliburApi({
      baseUrl: "http://api.example",
      token: "session-token",
      fetcher,
    });

    await api.createOrg({ name: "Excalibur", slug: "excalibur" });

    const [url, init] = fetcher.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("http://api.example/api/v1/orgs");
    expect(init.method).toBe("POST");
    expect(init.credentials).toBe("include");
    expect(new Headers(init.headers).get("authorization")).toBe("Bearer session-token");
    expect(new Headers(init.headers).get("content-type")).toBe("application/json");
    expect(init.body).toBe(JSON.stringify({ name: "Excalibur", slug: "excalibur" }));
  });

  it("throws API errors with server messages", async () => {
    const fetcher = vi.fn(
      async () =>
        new Response(JSON.stringify({ error: "tenant scope violation" }), {
          status: 401,
        }),
    );
    const api = createExcaliburApi({ baseUrl: "http://api.example", fetcher });

    await expect(api.listDevices("project-1")).rejects.toMatchObject({
      status: 401,
      message: "tenant scope violation",
    } satisfies Partial<ExcaliburApiError>);
  });

  it("preserves auth refresh token response contracts", async () => {
    const auth = {
      token: "xclb_access_token",
      refresh_token: "xclb_refresh_token",
      expires_at: "2026-07-30T12:00:00Z",
      refresh_expires_at: "2026-08-29T12:00:00Z",
      user_id: "user-1",
    };
    const fetcher = vi.fn(async () => new Response(JSON.stringify(auth)));
    const api = createExcaliburApi({ baseUrl: "http://api.example", fetcher });

    await expect(
      api.register({
        email: "ops@example.com",
        password: "correct horse battery staple",
        display_name: "Ops",
      }),
    ).resolves.toEqual(auth);
    await expect(api.login({ email: "ops@example.com", password: "correct horse battery staple" })).resolves.toEqual(
      auth,
    );
    await expect(api.refreshSession({ refresh_token: auth.refresh_token })).resolves.toEqual(auth);
    await expect(api.refreshSession()).resolves.toEqual(auth);

    const calls = (fetcher.mock.calls as unknown as [string, RequestInit][]).map(([url, init]) => ({
      url: String(url),
      method: init?.method ?? "GET",
      body: init?.body,
      credentials: init?.credentials,
    }));
    expect(calls).toMatchObject([
      {
        url: "http://api.example/api/v1/auth/register",
        method: "POST",
        credentials: "include",
        body: JSON.stringify({
          email: "ops@example.com",
          password: "correct horse battery staple",
          display_name: "Ops",
        }),
      },
      {
        url: "http://api.example/api/v1/auth/login",
        method: "POST",
        credentials: "include",
        body: JSON.stringify({
          email: "ops@example.com",
          password: "correct horse battery staple",
        }),
      },
      {
        url: "http://api.example/api/v1/auth/refresh",
        method: "POST",
        credentials: "include",
        body: JSON.stringify({ refresh_token: "xclb_refresh_token" }),
      },
      {
        url: "http://api.example/api/v1/auth/refresh",
        method: "POST",
        credentials: "include",
      },
    ]);
  });

  it("covers logout and API key endpoint contracts", async () => {
    const apiKeyBase = {
      id: "api-key-1",
      org_id: "org-1",
      project_id: "project-1",
      name: "worker ingest",
      scopes: ["telemetry:write"],
      expires_at: "2026-08-30T12:00:00Z",
      revoked_at: null,
      last_used_at: null,
      created_by: "user-1",
      created_at: "2026-07-30T12:00:00Z",
    };
    const createdApiKey = {
      ...apiKeyBase,
      key: "xclb_api_secret",
    };
    const fetcher = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ status: "logged_out" })))
      .mockResolvedValueOnce(new Response(JSON.stringify([apiKeyBase])))
      .mockResolvedValueOnce(new Response(JSON.stringify(createdApiKey)))
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ ...apiKeyBase, revoked_at: "2026-07-30T12:30:00Z" })),
      );
    const api = createExcaliburApi({
      baseUrl: "http://api.example",
      token: "session-token",
      fetcher,
    });

    await expect(api.logout()).resolves.toEqual({ status: "logged_out" });
    await expect(api.listApiKeys("org-1", "project-1")).resolves.toEqual([apiKeyBase]);
    await expect(
      api.createApiKey({
        org_id: "org-1",
        project_id: "project-1",
        name: "worker ingest",
        scopes: ["telemetry:write"],
        expires_at: "2026-08-30T12:00:00Z",
      }),
    ).resolves.toEqual(createdApiKey);
    await expect(api.revokeApiKey("api-key-1", "org-1")).resolves.toMatchObject({
      id: "api-key-1",
      revoked_at: "2026-07-30T12:30:00Z",
    });

    const calls = (fetcher.mock.calls as unknown as [string, RequestInit][]).map(([url, init]) => ({
      url: String(url),
      method: init?.method ?? "GET",
      body: init?.body,
      authorization: new Headers(init.headers).get("authorization"),
    }));
    expect(calls).toMatchObject([
      {
        url: "http://api.example/api/v1/auth/logout",
        method: "POST",
        authorization: "Bearer session-token",
      },
      {
        url: "http://api.example/api/v1/api-keys?org_id=org-1&project_id=project-1",
        method: "GET",
        authorization: "Bearer session-token",
      },
      {
        url: "http://api.example/api/v1/api-keys",
        method: "POST",
        body: JSON.stringify({
          org_id: "org-1",
          project_id: "project-1",
          name: "worker ingest",
          scopes: ["telemetry:write"],
          expires_at: "2026-08-30T12:00:00Z",
        }),
        authorization: "Bearer session-token",
      },
      {
        url: "http://api.example/api/v1/api-keys/api-key-1/revoke?org_id=org-1",
        method: "POST",
        authorization: "Bearer session-token",
      },
    ]);
  });

  it("covers user, org, project, and membership management endpoint contracts", async () => {
    const user = {
      id: "user-1",
      email: "ops@example.com",
      display_name: "Ops",
      email_verified: false,
    };
    const org = {
      id: "org-1",
      name: "Acme",
      slug: "acme",
      created_at: "2026-07-30T12:00:00Z",
    };
    const project = {
      id: "project-1",
      org_id: "org-1",
      name: "Factory",
      slug: "factory",
      created_at: "2026-07-30T12:00:00Z",
    };
    const membership = {
      id: "membership-1",
      org_id: "org-1",
      user_id: "user-2",
      role: "Viewer",
      email: "viewer@example.com",
      display_name: "Viewer",
      email_verified: true,
      created_at: "2026-07-30T12:00:00Z",
    };
    const fetcher = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify(user)))
      .mockResolvedValueOnce(new Response(JSON.stringify({ ...user, display_name: "Ops Lead" })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ org_id: "org-1", role: "Admin" })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ ...org, name: "Acme IoT" })))
      .mockResolvedValueOnce(new Response(JSON.stringify([membership])))
      .mockResolvedValueOnce(new Response(JSON.stringify(membership)))
      .mockResolvedValueOnce(new Response(JSON.stringify({ ...membership, role: "Operator" })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ ...membership, role: "Operator" })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ ...project, name: "Line 1" })));
    const api = createExcaliburApi({
      baseUrl: "http://api.example",
      token: "session-token",
      fetcher,
    });

    await expect(api.getMe()).resolves.toEqual(user);
    await expect(api.updateMe({ display_name: "Ops Lead" })).resolves.toMatchObject({
      display_name: "Ops Lead",
    });
    await expect(api.getOrgRole("org-1")).resolves.toEqual({ org_id: "org-1", role: "Admin" });
    await expect(api.updateOrg("org-1", { name: "Acme IoT" })).resolves.toMatchObject({
      name: "Acme IoT",
    });
    await expect(api.listMemberships("org-1")).resolves.toEqual([membership]);
    await expect(api.createMembership("org-1", { email: "viewer@example.com", role: "Viewer" })).resolves.toEqual(
      membership,
    );
    await expect(api.updateMembershipRole("org-1", "membership-1", { role: "Operator" })).resolves.toMatchObject({
      role: "Operator",
    });
    await expect(api.removeMembership("org-1", "membership-1")).resolves.toMatchObject({
      role: "Operator",
    });
    await expect(api.updateProject("project-1", { name: "Line 1" })).resolves.toMatchObject({
      name: "Line 1",
    });

    const calls = (fetcher.mock.calls as unknown as [string, RequestInit][]).map(([url, init]) => ({
      url: String(url),
      method: init?.method ?? "GET",
      body: init?.body,
      authorization: new Headers(init.headers).get("authorization"),
    }));
    expect(calls).toMatchObject([
      {
        url: "http://api.example/api/v1/me",
        method: "GET",
        authorization: "Bearer session-token",
      },
      {
        url: "http://api.example/api/v1/me",
        method: "PATCH",
        body: JSON.stringify({ display_name: "Ops Lead" }),
      },
      {
        url: "http://api.example/api/v1/orgs/org-1/role",
        method: "GET",
      },
      {
        url: "http://api.example/api/v1/orgs/org-1",
        method: "PATCH",
        body: JSON.stringify({ name: "Acme IoT" }),
      },
      {
        url: "http://api.example/api/v1/orgs/org-1/memberships",
        method: "GET",
      },
      {
        url: "http://api.example/api/v1/orgs/org-1/memberships",
        method: "POST",
        body: JSON.stringify({ email: "viewer@example.com", role: "Viewer" }),
      },
      {
        url: "http://api.example/api/v1/orgs/org-1/memberships/membership-1",
        method: "PATCH",
        body: JSON.stringify({ role: "Operator" }),
      },
      {
        url: "http://api.example/api/v1/orgs/org-1/memberships/membership-1",
        method: "DELETE",
      },
      {
        url: "http://api.example/api/v1/projects/project-1",
        method: "PATCH",
        body: JSON.stringify({ name: "Line 1" }),
      },
    ]);
  });

  it("preserves HTTP status for non-JSON API errors", async () => {
    const fetcher = vi.fn(
      async () =>
        new Response("<html>bad gateway</html>", {
          status: 502,
          headers: { "content-type": "text/html" },
        }),
    );
    const api = createExcaliburApi({ baseUrl: "http://api.example", fetcher });

    await expect(api.listOrgs()).rejects.toMatchObject({
      status: 502,
      message: "API request failed with 502",
      body: "<html>bad gateway</html>",
    } satisfies Partial<ExcaliburApiError>);
  });

  it("covers first-loop endpoint contracts", async () => {
    const fetcher = vi.fn(async () => new Response("{}"));
    const api = createExcaliburApi({
      baseUrl: "http://api.example",
      token: "session-token",
      fetcher,
    });

    await api.register({
      email: "ops@example.com",
      password: "correct horse battery staple",
      display_name: "Ops",
    });
    await api.listProjects("org-1");
    await api.provisionDevAuth("device-1", "project-1");
    await api.createFirmware({
      project_id: "project-1",
      component: "main",
      version: "1.0.0",
      object_key: "projects/project-1/firmware/main.bin",
      sha256: "a".repeat(64),
      content_type: "application/octet-stream",
      signature: "ed25519:test",
      size_bytes: 1024,
    });
    await api.createFirmwareUploadUrl("firmware-1", "project-1");
    await api.createFirmwareDownloadUrl("firmware-1", "project-1");
    await api.finalizeFirmware("firmware-1", {
      project_id: "project-1",
      sha256: "a".repeat(64),
      signature: "ed25519:test",
      size_bytes: 1024,
    });
    await api.createFirmwareRollout("firmware-1", {
      project_id: "project-1",
      device_ids: ["device-1"],
      requires_approval: true,
      rollback_strategy: "previous_version",
    });
    await api.listFirmwareRollouts("project-1");
    await api.ingestTelemetry({
      topic: "v1/p/project-1/d/device-1/telemetry/device_agent_system_stats",
      payload: [{ sequence: 1, timestamp: "2026-07-29T00:00:00Z", cpu_percent: 42 }],
    });
    await api.aggregateTelemetry({
      projectId: "project-1",
      deviceId: "device-1",
      stream: "device_agent_system_stats",
      field: "cpu_percent",
      bucketSeconds: 60,
    });
    await api.updateActionStatus("action-1", {
      project_id: "project-1",
      device_id: "device-1",
      state: "Completed",
      progress: 100,
      errors: [],
    });
    await api.approveAction("action-1", { project_id: "project-1" });
    await api.retryAction("action-1", { project_id: "project-1", device_ids: ["device-1"] });
    await api.cancelAction("action-1", {
      project_id: "project-1",
      reason: "operator cancelled rollout",
    });
    await api.createAlert({
      project_id: "project-1",
      name: "offline > 10m",
      kind: "Offline",
      expression: { window: "10m" },
    });
    await api.listAlertEvents("project-1", "Firing");
    await api.createDiagnosticsSession({
      project_id: "project-1",
      device_id: "device-1",
      paths: ["/var/log"],
      include_logs: true,
    });
    await api.listDiagnosticsSessions("project-1");
    await api.finalizeDiagnosticsSession("diagnostics-1", {
      project_id: "project-1",
      size_bytes: 2048,
      sha256: "c".repeat(64),
    });
    await api.createDiagnosticsDownloadUrl("diagnostics-1", "project-1");
    await api.listAudit("org-1", "project-1");

    const calls = (fetcher.mock.calls as unknown as [string, RequestInit][]).map(([url, init]) => ({
      url: String(url),
      method: init?.method ?? "GET",
      body: init?.body,
    }));
    expect(calls).toMatchObject([
      {
        url: "http://api.example/api/v1/auth/register",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/projects?org_id=org-1",
        method: "GET",
      },
      {
        url: "http://api.example/api/v1/devices/device-1/provision/dev-auth",
        method: "POST",
        body: JSON.stringify({ project_id: "project-1" }),
      },
      {
        url: "http://api.example/api/v1/firmware",
        method: "POST",
        body: JSON.stringify({
          project_id: "project-1",
          component: "main",
          version: "1.0.0",
          object_key: "projects/project-1/firmware/main.bin",
          sha256: "a".repeat(64),
          content_type: "application/octet-stream",
          signature: "ed25519:test",
          size_bytes: 1024,
        }),
      },
      {
        url: "http://api.example/api/v1/firmware/firmware-1/upload-url?project_id=project-1",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/firmware/firmware-1/download-url?project_id=project-1",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/firmware/firmware-1/finalize",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/firmware/firmware-1/rollout",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/firmware-rollouts?project_id=project-1",
        method: "GET",
      },
      {
        url: "http://api.example/api/v1/telemetry",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/telemetry/aggregate?project_id=project-1&device_id=device-1&stream=device_agent_system_stats&field=cpu_percent&bucket_seconds=60",
        method: "GET",
      },
      {
        url: "http://api.example/api/v1/actions/action-1/status",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/actions/action-1/approve",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/actions/action-1/retry",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/actions/action-1/cancel",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/alerts",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/alert-events?project_id=project-1&state=Firing",
        method: "GET",
      },
      {
        url: "http://api.example/api/v1/diagnostics/sessions",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/diagnostics/sessions?project_id=project-1",
        method: "GET",
      },
      {
        url: "http://api.example/api/v1/diagnostics/sessions/diagnostics-1/finalize",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/diagnostics/sessions/diagnostics-1/download-url?project_id=project-1",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/audit?org_id=org-1&project_id=project-1",
        method: "GET",
      },
    ]);
  });
});
