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
    await api.ingestTelemetry({
      topic: "v1/p/project-1/d/device-1/telemetry/device_agent_system_stats",
      payload: [{ sequence: 1, timestamp: "2026-07-29T00:00:00Z", cpu_percent: 42 }],
    });
    await api.updateActionStatus("action-1", {
      project_id: "project-1",
      device_id: "device-1",
      state: "Completed",
      progress: 100,
      errors: [],
    });
    await api.createAlert({
      project_id: "project-1",
      name: "offline > 10m",
      kind: "Offline",
      expression: { window: "10m" },
    });
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
        url: "http://api.example/api/v1/telemetry",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/actions/action-1/status",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/alerts",
        method: "POST",
      },
      {
        url: "http://api.example/api/v1/audit?org_id=org-1&project_id=project-1",
        method: "GET",
      },
    ]);
  });
});
