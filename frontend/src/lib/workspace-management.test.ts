import { describe, expect, it } from "vitest";
import {
  getApiKeyScopePreset,
  getApiKeyStatus,
  slugifyWorkspaceName,
} from "./workspace-management";

describe("workspace management helpers", () => {
  it("generates stable ASCII slugs from names", () => {
    expect(slugifyWorkspaceName("Factory EV Line")).toBe("factory-ev-line");
    expect(slugifyWorkspaceName("  North / South -- QA  ")).toBe("north-south-qa");
    expect(slugifyWorkspaceName("!!!", "project")).toBe("project");
  });

  it("classifies API key status with revocation taking precedence", () => {
    const now = Date.parse("2026-07-31T12:00:00Z");

    expect(getApiKeyStatus({ expires_at: "2026-08-01T12:00:00Z", revoked_at: null }, now)).toBe("active");
    expect(getApiKeyStatus({ expires_at: "2026-07-31T11:59:59Z", revoked_at: null }, now)).toBe("expired");
    expect(
      getApiKeyStatus(
        {
          expires_at: "2026-08-01T12:00:00Z",
          revoked_at: "2026-07-31T11:00:00Z",
        },
        now,
      ),
    ).toBe("revoked");
  });

  it("maps scope presets to API key scopes", () => {
    expect(getApiKeyScopePreset("telemetry-ingest").scopes).toEqual(["telemetry:write"]);
    expect(getApiKeyScopePreset("device-operator").scopes).toContain("devices:provision");
    expect(getApiKeyScopePreset("project-readonly").scopes).toContain("audit:read");
  });
});
