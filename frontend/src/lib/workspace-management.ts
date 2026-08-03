import type { MembershipResponse, RoleResponse } from "./generated/api-types";

export type ApiKeyStatus = "active" | "expired" | "revoked";
export type MemberRole = RoleResponse;

export type ApiKeyStatusInput = {
  expires_at?: string | null;
  revoked_at?: string | null;
};

export const apiKeyScopePresets = [
  {
    id: "telemetry-ingest",
    label: "Telemetry ingest",
    description: "Write telemetry and shadows from automation.",
    scopes: ["telemetry:write"],
  },
  {
    id: "device-operator",
    label: "Device operator",
    description: "Provision devices and run OTA or diagnostics actions.",
    scopes: ["devices:read", "devices:write", "devices:provision", "actions:write", "firmware:read"],
  },
  {
    id: "project-readonly",
    label: "Project readonly",
    description: "Read fleet, telemetry, actions, alerts, and audit data.",
    scopes: [
      "projects:read",
      "devices:read",
      "streams:read",
      "telemetry:read",
      "actions:read",
      "firmware:read",
      "alerts:read",
      "audit:read",
    ],
  },
] as const;

export const memberRoles = [
  {
    id: "Owner",
    label: "Owner",
    description: "Org settings, security, members, and all resources.",
  },
  {
    id: "Admin",
    label: "Admin",
    description: "Project settings, member administration, devices, and rules.",
  },
  {
    id: "Operator",
    label: "Operator",
    description: "Provisioning, OTA, diagnostics, and action operations.",
  },
  {
    id: "Viewer",
    label: "Viewer",
    description: "Read-only fleet, telemetry, dashboards, and audit access.",
  },
] satisfies Array<{
  id: MemberRole;
  label: string;
  description: string;
}>;

export type ApiKeyScopePresetId = (typeof apiKeyScopePresets)[number]["id"];

export function getApiKeyScopePreset(presetId: ApiKeyScopePresetId) {
  return apiKeyScopePresets.find((preset) => preset.id === presetId) ?? apiKeyScopePresets[0];
}

export function getApiKeyStatus(apiKey: ApiKeyStatusInput, nowMs = Date.now()): ApiKeyStatus {
  if (apiKey.revoked_at) {
    return "revoked";
  }
  if (apiKey.expires_at) {
    const expiresAt = Date.parse(apiKey.expires_at);
    if (Number.isFinite(expiresAt) && expiresAt <= nowMs) {
      return "expired";
    }
  }
  return "active";
}

export function slugifyWorkspaceName(value: string, fallback = "workspace") {
  const slug = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48)
    .replace(/-+$/g, "");

  return slug || fallback;
}

export function validateWorkspaceSlug(value: string) {
  const slug = value.trim();
  if (!slug) {
    return "Slug is required";
  }
  if (slug.length > 48) {
    return "Slug must be 48 characters or fewer";
  }
  if (slug.startsWith("-") || slug.endsWith("-")) {
    return "Slug cannot start or end with '-'";
  }
  if (!/^[a-z0-9-]+$/.test(slug)) {
    return "Slug must contain only lowercase letters, numbers, and '-'";
  }
  return null;
}

export function canEditOrganization(role: MemberRole | null | undefined) {
  return role === "Owner";
}

export function canEditProject(role: MemberRole | null | undefined) {
  return role === "Owner" || role === "Admin";
}

export function canManageMembers(role: MemberRole | null | undefined) {
  return role === "Owner" || role === "Admin";
}

export function canAssignMemberRole(actorRole: MemberRole | null | undefined, nextRole: MemberRole) {
  if (!canManageMembers(actorRole)) {
    return false;
  }
  return actorRole === "Owner" || nextRole !== "Owner";
}

export function canChangeMemberRole(
  actorRole: MemberRole | null | undefined,
  currentRole: MemberRole,
  nextRole: MemberRole,
) {
  if (!canManageMembers(actorRole)) {
    return false;
  }
  return actorRole === "Owner" || (currentRole !== "Owner" && nextRole !== "Owner");
}

export function canRemoveMember(actorRole: MemberRole | null | undefined, targetRole: MemberRole) {
  if (!canManageMembers(actorRole)) {
    return false;
  }
  return actorRole === "Owner" || targetRole !== "Owner";
}

export function isLastOwner(memberships: MembershipResponse[], membershipId: string) {
  const target = memberships.find((membership) => membership.id === membershipId);
  if (target?.role !== "Owner") {
    return false;
  }
  return memberships.filter((membership) => membership.role === "Owner").length <= 1;
}
