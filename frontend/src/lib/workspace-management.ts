export type ApiKeyStatus = "active" | "expired" | "revoked";

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
