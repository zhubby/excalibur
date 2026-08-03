"use client";

import { type FormEvent, useMemo, useState } from "react";
import {
  AlertTriangle,
  Building2,
  CheckCircle2,
  Clipboard,
  FolderKanban,
  KeyRound,
  Plus,
  ScrollText,
  ShieldCheck,
  Trash2,
  Users,
} from "lucide-react";
import type { ApiKey, AuditLog, Org, Project } from "@/lib/api";
import {
  apiKeyScopePresets,
  getApiKeyScopePreset,
  getApiKeyStatus,
  slugifyWorkspaceName,
  type ApiKeyScopePresetId,
} from "@/lib/workspace-management";

export type ApiKeyCreateInput = {
  name: string;
  presetId: ApiKeyScopePresetId;
  expiresInDays: number | null;
};

const apiKeyStatusClass = {
  active: "bg-success/15 text-success",
  expired: "bg-warning/15 text-warning",
  revoked: "bg-danger/15 text-danger",
} as const;

const roles = [
  { name: "Owner", scope: "Org management, security settings, and all resources." },
  { name: "Admin", scope: "Project management, member administration, devices, and rules." },
  { name: "Operator", scope: "Provisioning, OTA, diagnostics, and action operations." },
  { name: "Viewer", scope: "Read-only fleet, telemetry, dashboards, and audit access." },
];

export function formatDateTime(value: string | null | undefined) {
  if (!value) {
    return "Never";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return "Unknown";
  }
  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

export function OrganizationPanel({
  currentOrg,
  orgs,
  busy = false,
  onCreateOrg,
  onSelectOrg,
}: {
  currentOrg: Org;
  orgs: Org[];
  busy?: boolean;
  onCreateOrg: (name: string) => void;
  onSelectOrg: (orgId: string) => void;
}) {
  const [orgName, setOrgName] = useState("");
  const orgSlugPreview = slugifyWorkspaceName(orgName, "org");

  const submitOrg = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = orgName.trim();
    if (!name) {
      return;
    }
    onCreateOrg(name);
    setOrgName("");
  };

  return (
    <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
      <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
        <div>
          <h2 className="text-base font-semibold text-ink">Organization</h2>
          <p className="text-sm text-muted">Tenant boundary and accessible orgs.</p>
        </div>
        <Building2 className="h-5 w-5 text-brand" aria-hidden="true" />
      </div>
      <div className="space-y-4 p-4">
        <div className="rounded-md border border-line bg-elevated p-3">
          <p className="text-sm font-semibold text-ink">{currentOrg.name}</p>
          <p className="mt-1 break-all text-xs text-faint">{currentOrg.id}</p>
          <p className="mt-2 text-xs text-muted">/{currentOrg.slug}</p>
        </div>

        <div className="space-y-2">
          {orgs.map((org) => (
            <button
              key={org.id}
              className={`flex w-full items-center justify-between gap-3 rounded-md border px-3 py-2.5 text-left transition ${
                org.id === currentOrg.id
                  ? "border-brand/40 bg-brand/10 text-ink"
                  : "border-line bg-elevated text-muted hover:border-faint hover:text-ink"
              }`}
              type="button"
              disabled={busy || org.id === currentOrg.id}
              onClick={() => onSelectOrg(org.id)}
            >
              <span className="min-w-0">
                <span className="block truncate text-sm font-medium">{org.name}</span>
                <span className="block truncate text-xs text-faint">/{org.slug}</span>
              </span>
              {org.id === currentOrg.id ? (
                <span className="rounded-sm bg-success/15 px-2 py-1 text-xs font-medium text-success">Current</span>
              ) : (
                <span className="text-xs text-faint">Open</span>
              )}
            </button>
          ))}
        </div>

        <form className="flex flex-col gap-2 sm:flex-row" onSubmit={submitOrg}>
          <label className="min-w-0 flex-1">
            <span className="sr-only">Organization name</span>
            <input
              className="h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink placeholder:text-faint transition hover:border-faint"
              value={orgName}
              onChange={(event) => setOrgName(event.target.value)}
              placeholder="New organization"
              type="text"
            />
          </label>
          <button
            className="inline-flex h-10 items-center justify-center gap-2 rounded-md bg-brand px-3 text-sm font-medium text-ink transition hover:bg-brand-hover disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint"
            type="submit"
            disabled={busy || !orgName.trim()}
          >
            <Plus className="h-4 w-4" aria-hidden="true" />
            <span>Create</span>
          </button>
        </form>
        <p className="truncate text-xs text-faint">Slug preview: /{orgSlugPreview}</p>
      </div>
    </section>
  );
}

export function ProjectsPanel({
  currentProject,
  projects,
  busy = false,
  onCreateProject,
  onSelectProject,
}: {
  currentProject: Project;
  projects: Project[];
  busy?: boolean;
  onCreateProject: (name: string) => void;
  onSelectProject: (projectId: string) => void;
}) {
  const [projectName, setProjectName] = useState("");
  const projectSlugPreview = slugifyWorkspaceName(projectName, "project");

  const submitProject = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = projectName.trim();
    if (!name) {
      return;
    }
    onCreateProject(name);
    setProjectName("");
  };

  return (
    <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
      <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
        <div>
          <h2 className="text-base font-semibold text-ink">Projects</h2>
          <p className="text-sm text-muted">Fleet, stream, firmware, and alert isolation.</p>
        </div>
        <FolderKanban className="h-5 w-5 text-brand" aria-hidden="true" />
      </div>
      <div className="space-y-4 p-4">
        <div className="space-y-2">
          {projects.map((project) => (
            <button
              key={project.id}
              className={`flex w-full items-center justify-between gap-3 rounded-md border px-3 py-2.5 text-left transition ${
                project.id === currentProject.id
                  ? "border-brand/40 bg-brand/10 text-ink"
                  : "border-line bg-elevated text-muted hover:border-faint hover:text-ink"
              }`}
              type="button"
              disabled={busy || project.id === currentProject.id}
              onClick={() => onSelectProject(project.id)}
            >
              <span className="min-w-0">
                <span className="block truncate text-sm font-medium">{project.name}</span>
                <span className="block truncate text-xs text-faint">/{project.slug}</span>
              </span>
              {project.id === currentProject.id ? (
                <span className="rounded-sm bg-success/15 px-2 py-1 text-xs font-medium text-success">Active</span>
              ) : (
                <span className="text-xs text-faint">Switch</span>
              )}
            </button>
          ))}
          {projects.length === 0 ? (
            <div className="rounded-md border border-line bg-elevated px-3 py-6 text-center text-sm text-muted">
              No projects in this org.
            </div>
          ) : null}
        </div>

        <form className="flex flex-col gap-2 sm:flex-row" onSubmit={submitProject}>
          <label className="min-w-0 flex-1">
            <span className="sr-only">Project name</span>
            <input
              className="h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink placeholder:text-faint transition hover:border-faint"
              value={projectName}
              onChange={(event) => setProjectName(event.target.value)}
              placeholder="New project"
              type="text"
            />
          </label>
          <button
            className="inline-flex h-10 items-center justify-center gap-2 rounded-md bg-brand px-3 text-sm font-medium text-ink transition hover:bg-brand-hover disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint"
            type="submit"
            disabled={busy || !projectName.trim()}
          >
            <Plus className="h-4 w-4" aria-hidden="true" />
            <span>Create</span>
          </button>
        </form>
        <p className="truncate text-xs text-faint">Slug preview: /{projectSlugPreview}</p>
      </div>
    </section>
  );
}

export function PermissionsPanel({
  apiKeys,
  apiKeyError,
  createdApiKey,
  busy = false,
  onCreateApiKey,
  onRevokeApiKey,
}: {
  apiKeys: ApiKey[];
  apiKeyError: string | null;
  createdApiKey: ApiKey | null;
  busy?: boolean;
  onCreateApiKey: (input: ApiKeyCreateInput) => void;
  onRevokeApiKey: (apiKeyId: string) => void;
}) {
  const [apiKeyName, setApiKeyName] = useState("");
  const [scopePresetId, setScopePresetId] = useState<ApiKeyScopePresetId>("telemetry-ingest");
  const [expiresInDays, setExpiresInDays] = useState("30");
  const [expiryMode, setExpiryMode] = useState<"days" | "never">("days");
  const [copied, setCopied] = useState(false);
  const selectedPreset = useMemo(() => getApiKeyScopePreset(scopePresetId), [scopePresetId]);
  const expiresInDaysValue = Number.parseInt(expiresInDays, 10);
  const hasValidExpiryDays = Number.isFinite(expiresInDaysValue) && expiresInDaysValue > 0;
  const canCreateApiKey =
    !busy && !apiKeyError && Boolean(apiKeyName.trim()) && (expiryMode === "never" || hasValidExpiryDays);

  const submitApiKey = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = apiKeyName.trim();
    if (!name) {
      return;
    }
    if (expiryMode === "days" && !hasValidExpiryDays) {
      return;
    }
    onCreateApiKey({
      name,
      presetId: scopePresetId,
      expiresInDays: expiryMode === "never" ? null : expiresInDaysValue,
    });
    setApiKeyName("");
  };

  const copyCreatedKey = () => {
    if (!createdApiKey?.key || !navigator.clipboard) {
      return;
    }
    void navigator.clipboard
      .writeText(createdApiKey.key)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1400);
      })
      .catch(() => setCopied(false));
  };

  return (
    <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
      <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
        <div>
          <h2 className="text-base font-semibold text-ink">Permissions</h2>
          <p className="text-sm text-muted">API keys, scope presets, and RBAC reference.</p>
        </div>
        <KeyRound className="h-5 w-5 text-brand" aria-hidden="true" />
      </div>
      <div className="grid gap-5 p-4 xl:grid-cols-[minmax(0,1fr)_360px]">
        <div className="min-w-0 space-y-4">
          <form className="grid gap-3 rounded-md border border-line bg-rail p-3 md:grid-cols-[minmax(0,1fr)_220px_140px_auto]" onSubmit={submitApiKey}>
            <label className="min-w-0">
              <span className="mb-1 block text-xs font-medium text-faint">Name</span>
              <input
                className="h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink placeholder:text-faint transition hover:border-faint"
                value={apiKeyName}
                onChange={(event) => setApiKeyName(event.target.value)}
                placeholder="worker ingest"
                type="text"
              />
            </label>
            <label>
              <span className="mb-1 block text-xs font-medium text-faint">Scope preset</span>
              <select
                className="h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
                value={scopePresetId}
                onChange={(event) => setScopePresetId(event.target.value as ApiKeyScopePresetId)}
              >
                {apiKeyScopePresets.map((preset) => (
                  <option key={preset.id} value={preset.id}>
                    {preset.label}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span className="mb-1 block text-xs font-medium text-faint">Expires in days</span>
              <input
                className="h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
                value={expiresInDays}
                onChange={(event) => setExpiresInDays(event.target.value)}
                disabled={expiryMode === "never"}
                min={1}
                type="number"
              />
            </label>
            <button
              className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md bg-brand px-3 text-sm font-medium text-ink transition hover:bg-brand-hover disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint"
              type="submit"
              disabled={!canCreateApiKey}
            >
              <Plus className="h-4 w-4" aria-hidden="true" />
              <span>Create key</span>
            </button>
          </form>
          <label className="inline-flex items-center gap-2 text-sm text-muted">
            <input
              className="h-4 w-4 rounded-sm border-line bg-elevated"
              checked={expiryMode === "never"}
              onChange={(event) => setExpiryMode(event.target.checked ? "never" : "days")}
              type="checkbox"
            />
            <span>Never expires</span>
          </label>
          <p className="text-xs text-faint">
            {selectedPreset.description} Scopes: {selectedPreset.scopes.join(", ")}
          </p>

          {createdApiKey?.key ? (
            <div className="rounded-md border border-success/25 bg-success/10 p-3">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <p className="text-sm font-semibold text-success">API key created</p>
                  <p className="text-xs text-muted">Store this value now. The API will not return it again.</p>
                </div>
                <button
                  className="inline-flex h-8 items-center justify-center gap-2 rounded-md border border-success/30 bg-panel px-2 text-xs font-medium text-success transition hover:bg-elevated"
                  type="button"
                  onClick={copyCreatedKey}
                >
                  <Clipboard className="h-3.5 w-3.5" aria-hidden="true" />
                  <span>{copied ? "Copied" : "Copy"}</span>
                </button>
              </div>
              <code className="mt-3 block break-all rounded-sm bg-panel p-2 text-xs text-ink">{createdApiKey.key}</code>
            </div>
          ) : null}

          {apiKeyError ? (
            <div className="flex items-start gap-3 rounded-md border border-warning/30 bg-warning/10 p-3 text-sm text-warning">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              <p>Admin access is required for API key management: {apiKeyError}</p>
            </div>
          ) : null}

          <div className="overflow-x-auto rounded-md border border-line">
            <table className="min-w-[760px] w-full table-fixed border-collapse text-left text-sm">
              <thead className="bg-rail text-xs uppercase text-faint">
                <tr>
                  <th className="px-3 py-3 font-semibold">Name</th>
                  <th className="px-3 py-3 font-semibold">Scopes</th>
                  <th className="px-3 py-3 font-semibold">Expires</th>
                  <th className="px-3 py-3 font-semibold">Status</th>
                  <th className="px-3 py-3 font-semibold" aria-label="Actions" />
                </tr>
              </thead>
              <tbody className="divide-y divide-line">
                {apiKeys.map((apiKey) => {
                  const status = getApiKeyStatus(apiKey);
                  return (
                    <tr key={apiKey.id} className="bg-elevated/35">
                      <td className="px-3 py-3">
                        <p className="truncate font-medium text-ink">{apiKey.name}</p>
                        <p className="truncate text-xs text-faint">{apiKey.id}</p>
                      </td>
                      <td className="px-3 py-3 text-xs text-muted">
                        <span className="line-clamp-2">{apiKey.scopes.join(", ")}</span>
                      </td>
                      <td className="px-3 py-3 text-muted">{formatDateTime(apiKey.expires_at)}</td>
                      <td className="px-3 py-3">
                        <span className={`inline-flex rounded-sm px-2 py-1 text-xs font-medium ${apiKeyStatusClass[status]}`}>
                          {status}
                        </span>
                      </td>
                      <td className="px-3 py-3 text-right">
                        <button
                          className="inline-grid h-8 w-8 place-items-center rounded-md text-muted transition hover:bg-danger hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
                          type="button"
                          aria-label={`Revoke ${apiKey.name}`}
                          disabled={busy || status === "revoked"}
                          onClick={() => onRevokeApiKey(apiKey.id)}
                        >
                          <Trash2 className="h-4 w-4" aria-hidden="true" />
                        </button>
                      </td>
                    </tr>
                  );
                })}
                {apiKeys.length === 0 ? (
                  <tr>
                    <td className="px-3 py-8 text-center text-sm text-muted" colSpan={5}>
                      No API keys for this project.
                    </td>
                  </tr>
                ) : null}
              </tbody>
            </table>
          </div>
        </div>

        <aside className="space-y-3">
          <div className="rounded-md border border-line bg-rail p-3">
            <div className="flex items-center gap-2 text-sm font-semibold text-ink">
              <ShieldCheck className="h-4 w-4 text-success" aria-hidden="true" />
              <span>RBAC roles</span>
            </div>
            <div className="mt-3 space-y-2">
              {roles.map((role) => (
                <article key={role.name} className="rounded-md border border-line bg-elevated p-3">
                  <p className="text-sm font-semibold text-ink">{role.name}</p>
                  <p className="mt-1 text-xs text-muted">{role.scope}</p>
                </article>
              ))}
            </div>
          </div>
          <div className="rounded-md border border-line bg-elevated p-3">
            <div className="flex items-center gap-2 text-sm font-semibold text-ink">
              <Users className="h-4 w-4 text-brand" aria-hidden="true" />
              <span>Members</span>
            </div>
            <p className="mt-2 text-xs text-muted">
              Member invitations and role changes need backend membership endpoints before they can be managed here.
            </p>
          </div>
        </aside>
      </div>
    </section>
  );
}

export function AuditLogPanel({ audit }: { audit: AuditLog[] }) {
  return (
    <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
      <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
        <div>
          <h2 className="text-base font-semibold text-ink">Audit log</h2>
          <p className="text-sm text-muted">Recent org and project scoped control-plane writes.</p>
        </div>
        <ScrollText className="h-5 w-5 text-brand" aria-hidden="true" />
      </div>
      <div className="divide-y divide-line">
        {audit.slice(0, 12).map((entry) => (
          <article key={entry.id} className="grid gap-2 px-4 py-3 md:grid-cols-[220px_minmax(0,1fr)_220px] md:items-center">
            <div className="min-w-0">
              <p className="truncate text-sm font-semibold text-ink">{entry.action}</p>
              <p className="truncate text-xs text-faint">{formatDateTime(entry.created_at)}</p>
            </div>
            <p className="truncate text-sm text-muted">{entry.resource}</p>
            <div className="flex min-w-0 items-center gap-2 text-xs text-faint">
              <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-success" aria-hidden="true" />
              <span className="truncate">{entry.project_id ? "project scoped" : "org scoped"}</span>
            </div>
          </article>
        ))}
        {audit.length === 0 ? (
          <div className="px-4 py-8 text-center text-sm text-muted">No audit entries yet.</div>
        ) : null}
      </div>
    </section>
  );
}
