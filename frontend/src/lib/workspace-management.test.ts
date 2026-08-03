import { describe, expect, it } from "vitest";
import {
  canAssignMemberRole,
  canChangeMemberRole,
  canEditOrganization,
  canEditProject,
  canManageMembers,
  canRemoveMember,
  getApiKeyScopePreset,
  getApiKeyStatus,
  isLastOwner,
  slugifyWorkspaceName,
  validateWorkspaceSlug,
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

  it("validates server-compatible workspace slugs", () => {
    expect(validateWorkspaceSlug("factory-line")).toBeNull();
    expect(validateWorkspaceSlug("")).toBe("Slug is required");
    expect(validateWorkspaceSlug("-factory")).toBe("Slug cannot start or end with '-'");
    expect(validateWorkspaceSlug("Factory")).toBe("Slug must contain only lowercase letters, numbers, and '-'");
    expect(validateWorkspaceSlug("a".repeat(49))).toBe("Slug must be 48 characters or fewer");
  });

  it("maps role capabilities for workspace management", () => {
    expect(canEditOrganization("Owner")).toBe(true);
    expect(canEditOrganization("Admin")).toBe(false);
    expect(canEditProject("Admin")).toBe(true);
    expect(canEditProject("Viewer")).toBe(false);
    expect(canManageMembers("Operator")).toBe(false);
    expect(canAssignMemberRole("Admin", "Owner")).toBe(false);
    expect(canAssignMemberRole("Admin", "Viewer")).toBe(true);
    expect(canChangeMemberRole("Admin", "Owner", "Admin")).toBe(false);
    expect(canChangeMemberRole("Owner", "Owner", "Admin")).toBe(true);
    expect(canRemoveMember("Admin", "Owner")).toBe(false);
    expect(canRemoveMember("Admin", "Viewer")).toBe(true);
  });

  it("detects last owner membership changes", () => {
    const memberships = [
      {
        id: "membership-owner",
        org_id: "org-1",
        user_id: "user-owner",
        role: "Owner",
        email: "owner@example.com",
        display_name: "Owner",
        email_verified: true,
        created_at: "2026-07-30T12:00:00Z",
      },
      {
        id: "membership-viewer",
        org_id: "org-1",
        user_id: "user-viewer",
        role: "Viewer",
        email: "viewer@example.com",
        display_name: "Viewer",
        email_verified: true,
        created_at: "2026-07-30T12:00:00Z",
      },
    ] as const;

    expect(isLastOwner([...memberships], "membership-owner")).toBe(true);
    expect(isLastOwner([...memberships, { ...memberships[0], id: "membership-owner-2" }], "membership-owner")).toBe(false);
    expect(isLastOwner([...memberships], "membership-viewer")).toBe(false);
  });
});
