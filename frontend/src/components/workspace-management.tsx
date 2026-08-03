"use client";

import { FormEvent, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  Building2,
  CheckCircle2,
  Clipboard,
  FolderKanban,
  KeyRound,
  Plus,
  Save,
  ScrollText,
  ShieldCheck,
  Trash2,
  UserCircle,
  Users,
} from "lucide-react";
import type { ApiKey, AuditLog, CurrentUser, MemberRole, Membership, Org, Project } from "@/lib/api";
import { managementNavItems, type ManagementSectionId } from "@/lib/data";
import {
  apiKeyScopePresets,
  canAssignMemberRole,
  canChangeMemberRole,
  canEditOrganization,
  canEditProject,
  canManageMembers,
  canRemoveMember,
  getApiKeyScopePreset,
  getApiKeyStatus,
  isLastOwner,
  memberRoles,
  slugifyWorkspaceName,
  validateWorkspaceSlug,
  type ApiKeyScopePresetId,
} from "@/lib/workspace-management";

export type ApiKeyCreateInput = {
  name: string;
  presetId: ApiKeyScopePresetId;
  expiresInDays: number | null;
};

export type MembershipCreateInput = {
  email: string;
  role: MemberRole;
};

type WorkspaceManagementProps = {
  activeView: ManagementSectionId;
  currentUser: CurrentUser | null;
  currentOrgRole: MemberRole | null;
  currentOrg: Org;
  currentProject: Project | null;
  orgs: Org[];
  projects: Project[];
  memberships: Membership[];
  membershipError: string | null;
  apiKeys: ApiKey[];
  apiKeyError: string | null;
  audit: AuditLog[];
  createdApiKey: ApiKey | null;
  busy?: boolean;
  setSectionRef: (section: ManagementSectionId) => (node: HTMLElement | null) => void;
  onViewChange: (section: ManagementSectionId) => void;
  onUpdateUser: (displayName: string) => void;
  onCreateOrg: (name: string) => void;
  onSelectOrg: (orgId: string) => void;
  onUpdateOrg: (orgId: string, input: { name: string; slug: string }) => void;
  onCreateProject: (name: string) => void;
  onSelectProject: (projectId: string) => void;
  onUpdateProject: (projectId: string, input: { name: string; slug: string }) => void;
  onCreateMembership: (input: MembershipCreateInput) => boolean | Promise<boolean>;
  onUpdateMembershipRole: (membershipId: string, role: MemberRole) => void;
  onRemoveMembership: (membershipId: string) => void;
  onCreateApiKey: (input: ApiKeyCreateInput) => void;
  onRevokeApiKey: (apiKeyId: string) => void;
};

const apiKeyStatusClass = {
  active: "bg-success/15 text-success",
  expired: "bg-warning/15 text-warning",
  revoked: "bg-danger/15 text-danger",
} as const;

function formatDateTime(value: string | null | undefined) {
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

function displayRole(role: MemberRole | null | undefined) {
  return role ?? "Viewer";
}

function slugError(value: string) {
  return validateWorkspaceSlug(value);
}

export function WorkspaceManagement({
  activeView,
  currentUser,
  currentOrgRole,
  currentOrg,
  currentProject,
  orgs,
  projects,
  memberships,
  membershipError,
  apiKeys,
  apiKeyError,
  audit,
  createdApiKey,
  busy = false,
  setSectionRef,
  onViewChange,
  onUpdateUser,
  onCreateOrg,
  onSelectOrg,
  onUpdateOrg,
  onCreateProject,
  onSelectProject,
  onUpdateProject,
  onCreateMembership,
  onUpdateMembershipRole,
  onRemoveMembership,
  onCreateApiKey,
  onRevokeApiKey,
}: WorkspaceManagementProps) {
  const [accountName, setAccountName] = useState(currentUser?.display_name ?? "");
  const [orgName, setOrgName] = useState("");
  const [projectName, setProjectName] = useState("");
  const [selectedOrgId, setSelectedOrgId] = useState(currentOrg.id);
  const [selectedProjectId, setSelectedProjectId] = useState(currentProject?.id ?? projects[0]?.id ?? "");
  const [orgDraft, setOrgDraft] = useState({ name: currentOrg.name, slug: currentOrg.slug });
  const [projectDraft, setProjectDraft] = useState({
    name: currentProject?.name ?? projects[0]?.name ?? "",
    slug: currentProject?.slug ?? projects[0]?.slug ?? "",
  });
  const [memberEmail, setMemberEmail] = useState("");
  const [memberRole, setMemberRole] = useState<MemberRole>("Viewer");
  const [apiKeyName, setApiKeyName] = useState("");
  const [scopePresetId, setScopePresetId] = useState<ApiKeyScopePresetId>("telemetry-ingest");
  const [expiresInDays, setExpiresInDays] = useState("30");
  const [expiryMode, setExpiryMode] = useState<"days" | "never">("days");
  const [copied, setCopied] = useState(false);

  const selectedOrg = orgs.find((org) => org.id === selectedOrgId) ?? currentOrg;
  const selectedProject =
    projects.find((project) => project.id === selectedProjectId) ?? currentProject ?? projects[0] ?? null;
  const selectedPreset = useMemo(() => getApiKeyScopePreset(scopePresetId), [scopePresetId]);
  const expiresInDaysValue = Number.parseInt(expiresInDays, 10);
  const hasValidExpiryDays = Number.isFinite(expiresInDaysValue) && expiresInDaysValue > 0;
  const canCreateApiKey =
    !busy &&
    Boolean(currentProject) &&
    !apiKeyError &&
    Boolean(apiKeyName.trim()) &&
    (expiryMode === "never" || hasValidExpiryDays);
  const canUpdateAccount = !busy && Boolean(currentUser) && Boolean(accountName.trim());
  const canUpdateSelectedOrg =
    !busy &&
    selectedOrg.id === currentOrg.id &&
    canEditOrganization(currentOrgRole) &&
    Boolean(orgDraft.name.trim()) &&
    !slugError(orgDraft.slug);
  const canUpdateSelectedProject =
    !busy &&
    Boolean(selectedProject) &&
    canEditProject(currentOrgRole) &&
    Boolean(projectDraft.name.trim()) &&
    !slugError(projectDraft.slug);
  const canCreateMember = !busy && canAssignMemberRole(currentOrgRole, memberRole) && Boolean(memberEmail.trim());
  const orgSlugPreview = slugifyWorkspaceName(orgName, "org");
  const projectSlugPreview = slugifyWorkspaceName(projectName, "project");

  const setManagementRootRef = (node: HTMLElement | null) => {
    managementNavItems.forEach((item) => setSectionRef(item.id)(node));
  };

  useEffect(() => {
    setAccountName(currentUser?.display_name ?? "");
  }, [currentUser?.display_name]);

  useEffect(() => {
    setSelectedOrgId(currentOrg.id);
    setOrgDraft({ name: currentOrg.name, slug: currentOrg.slug });
  }, [currentOrg.id, currentOrg.name, currentOrg.slug]);

  useEffect(() => {
    const nextProject = currentProject ?? projects[0] ?? null;
    setSelectedProjectId(nextProject?.id ?? "");
    setProjectDraft({ name: nextProject?.name ?? "", slug: nextProject?.slug ?? "" });
  }, [currentProject, projects]);

  const selectOrgForDetail = (org: Org) => {
    setSelectedOrgId(org.id);
    setOrgDraft({ name: org.name, slug: org.slug });
  };

  const selectProjectForDetail = (project: Project) => {
    setSelectedProjectId(project.id);
    setProjectDraft({ name: project.name, slug: project.slug });
  };

  const submitAccount = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const displayName = accountName.trim();
    if (!displayName) {
      return;
    }
    onUpdateUser(displayName);
  };

  const submitOrg = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = orgName.trim();
    if (!name) {
      return;
    }
    onCreateOrg(name);
    setOrgName("");
  };

  const submitOrgUpdate = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!canUpdateSelectedOrg) {
      return;
    }
    onUpdateOrg(selectedOrg.id, {
      name: orgDraft.name.trim(),
      slug: orgDraft.slug.trim(),
    });
  };

  const submitProject = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = projectName.trim();
    if (!name) {
      return;
    }
    onCreateProject(name);
    setProjectName("");
  };

  const submitProjectUpdate = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!selectedProject || !canUpdateSelectedProject) {
      return;
    }
    onUpdateProject(selectedProject.id, {
      name: projectDraft.name.trim(),
      slug: projectDraft.slug.trim(),
    });
  };

  const submitMembership = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const email = memberEmail.trim();
    if (!email || !canCreateMember) {
      return;
    }
    const created = await onCreateMembership({ email, role: memberRole });
    if (created) {
      setMemberEmail("");
      setMemberRole("Viewer");
    }
  };

  const changeMembershipRole = (membership: Membership, role: MemberRole) => {
    if (!canChangeMemberRole(currentOrgRole, membership.role, role) || isLastOwner(memberships, membership.id)) {
      return;
    }
    if (membership.user_id === currentUser?.id && !window.confirm("Change your own role?")) {
      return;
    }
    onUpdateMembershipRole(membership.id, role);
  };

  const removeMembership = (membership: Membership) => {
    if (!canRemoveMember(currentOrgRole, membership.role) || isLastOwner(memberships, membership.id)) {
      return;
    }
    const selfRemoval = membership.user_id === currentUser?.id;
    const message = selfRemoval
      ? "Remove your own membership from this organization?"
      : `Remove ${membership.email} from this organization?`;
    if (!window.confirm(message)) {
      return;
    }
    onRemoveMembership(membership.id);
  };

  const submitApiKey = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = apiKeyName.trim();
    if (!name || !canCreateApiKey) {
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
    <section
      className="panel-in rounded-md border border-line bg-panel shadow-panel"
      aria-label="User management center"
      ref={setManagementRootRef}
    >
      <div className="border-b border-line px-4 py-3">
        <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <h2 className="text-base font-semibold text-ink">Management center</h2>
            <p className="text-sm text-muted">
              Account, organization, project, member, API key, and audit management.
            </p>
          </div>
          <div className="inline-flex w-fit items-center gap-2 rounded-md border border-line bg-elevated px-3 py-2 text-sm text-muted">
            <ShieldCheck className="h-4 w-4 text-success" aria-hidden="true" />
            <span>{displayRole(currentOrgRole)}</span>
          </div>
        </div>
      </div>

      <div className="grid gap-0 lg:grid-cols-[230px_minmax(0,1fr)]">
        <nav className="overflow-x-auto border-b border-line bg-rail p-2 lg:border-b-0 lg:border-r" aria-label="Management views">
          <div className="flex min-w-max gap-1 lg:min-w-0 lg:flex-col">
            {managementNavItems.map((item) => (
              <button
                key={item.id}
                className={`flex h-10 items-center gap-2 rounded-md px-3 text-left text-sm transition lg:w-full ${
                  activeView === item.id ? "bg-brand text-ink" : "text-muted hover:bg-elevated hover:text-ink"
                }`}
                type="button"
                aria-current={activeView === item.id ? "page" : undefined}
                onClick={() => onViewChange(item.id)}
              >
                <item.icon className="h-4 w-4 shrink-0" aria-hidden="true" />
                <span>{item.label}</span>
              </button>
            ))}
          </div>
        </nav>

        <div className="min-w-0 p-4">{renderView()}</div>
      </div>
    </section>
  );

  function renderView() {
    switch (activeView) {
      case "account":
        return (
          <section className="max-w-2xl space-y-4" aria-label="Account settings">
            <ViewHeading icon={UserCircle} title="Account" description="Your console identity." />
            <form className="space-y-3" onSubmit={submitAccount}>
              <label className="block text-sm font-medium text-muted">
                Email
                <input
                  className="mt-1 h-10 w-full rounded-md border border-line bg-rail px-3 text-sm text-faint"
                  value={currentUser?.email ?? ""}
                  readOnly
                  type="email"
                />
              </label>
              <label className="block text-sm font-medium text-muted">
                Display name
                <input
                  className="mt-1 h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
                  value={accountName}
                  onChange={(event) => setAccountName(event.target.value)}
                  type="text"
                />
              </label>
              <div className="flex items-center justify-between gap-3">
                <p className="text-xs text-faint">
                  Email verification: {currentUser?.email_verified ? "verified" : "not verified"}
                </p>
                <SaveButton disabled={!canUpdateAccount || accountName.trim() === currentUser?.display_name} />
              </div>
            </form>
          </section>
        );
      case "organizations":
        return (
          <section className="space-y-4" aria-label="Organization management">
            <ViewHeading icon={Building2} title="Organizations" description="Tenant boundaries available to you." />
            <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
              <div className="space-y-2">
                {orgs.map((org) => (
                  <ResourceRow
                    key={org.id}
                    active={org.id === currentOrg.id}
                    title={org.name}
                    subtitle={`/${org.slug}`}
                    meta={org.id}
                    activeLabel="Current"
                    actionLabel={org.id === selectedOrg.id ? "Open" : "Details"}
                    onOpen={() => selectOrgForDetail(org)}
                    onActivate={org.id === currentOrg.id ? undefined : () => onSelectOrg(org.id)}
                    busy={busy}
                  />
                ))}
              </div>

              <div className="space-y-4">
                <form className="rounded-md border border-line bg-rail p-3" onSubmit={submitOrg}>
                  <p className="text-sm font-semibold text-ink">Create organization</p>
                  <div className="mt-3 flex flex-col gap-2 sm:flex-row xl:flex-col">
                    <input
                      className="h-10 min-w-0 flex-1 rounded-md border border-line bg-elevated px-3 text-sm text-ink placeholder:text-faint transition hover:border-faint"
                      value={orgName}
                      onChange={(event) => setOrgName(event.target.value)}
                      placeholder="New organization"
                      type="text"
                    />
                    <button
                      className="inline-flex h-10 items-center justify-center gap-2 rounded-md bg-brand px-3 text-sm font-medium text-ink transition hover:bg-brand-hover disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint"
                      type="submit"
                      disabled={busy || !orgName.trim()}
                    >
                      <Plus className="h-4 w-4" aria-hidden="true" />
                      <span>Create</span>
                    </button>
                  </div>
                  <p className="mt-2 truncate text-xs text-faint">Slug preview: /{orgSlugPreview}</p>
                </form>

                <form className="rounded-md border border-line bg-elevated p-3" onSubmit={submitOrgUpdate}>
                  <div className="flex items-center justify-between gap-3">
                    <p className="truncate text-sm font-semibold text-ink">{selectedOrg.name}</p>
                    {selectedOrg.id !== currentOrg.id ? (
                      <button
                        className="h-8 rounded-md border border-line px-2 text-xs text-muted transition hover:bg-line hover:text-ink"
                        type="button"
                        disabled={busy}
                        onClick={() => onSelectOrg(selectedOrg.id)}
                      >
                        Set active
                      </button>
                    ) : null}
                  </div>
                  <div className="mt-3 space-y-3">
                    <TextField label="Name" value={orgDraft.name} onChange={(value) => setOrgDraft((draft) => ({ ...draft, name: value }))} />
                    <TextField label="Slug" value={orgDraft.slug} onChange={(value) => setOrgDraft((draft) => ({ ...draft, slug: value }))} />
                    {slugError(orgDraft.slug) ? <p className="text-xs text-warning">{slugError(orgDraft.slug)}</p> : null}
                    {selectedOrg.id !== currentOrg.id ? (
                      <p className="text-xs text-faint">Set this organization active before editing settings.</p>
                    ) : !canEditOrganization(currentOrgRole) ? (
                      <p className="text-xs text-faint">Owner role is required for organization settings.</p>
                    ) : null}
                    <SaveButton disabled={!canUpdateSelectedOrg} />
                  </div>
                </form>
              </div>
            </div>
          </section>
        );
      case "members":
        return (
          <section className="space-y-4" aria-label="Member management">
            <ViewHeading icon={Users} title="Members" description="Organization-level roles inherited by projects." />
            {membershipError ? (
              <InlineWarning message={`Admin access is required for member management: ${membershipError}`} />
            ) : null}
            <form className="grid gap-3 rounded-md border border-line bg-rail p-3 md:grid-cols-[minmax(0,1fr)_180px_auto]" onSubmit={submitMembership}>
              <label className="min-w-0">
                <span className="mb-1 block text-xs font-medium text-faint">Registered email</span>
                <input
                  className="h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink placeholder:text-faint transition hover:border-faint"
                  value={memberEmail}
                  onChange={(event) => setMemberEmail(event.target.value)}
                  placeholder="viewer@example.com"
                  type="email"
                />
              </label>
              <label>
                <span className="mb-1 block text-xs font-medium text-faint">Role</span>
                <select
                  className="h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
                  value={memberRole}
                  onChange={(event) => setMemberRole(event.target.value as MemberRole)}
                >
                  {memberRoles.map((role) => (
                    <option key={role.id} value={role.id} disabled={!canAssignMemberRole(currentOrgRole, role.id)}>
                      {role.label}
                    </option>
                  ))}
                </select>
              </label>
              <button
                className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md bg-brand px-3 text-sm font-medium text-ink transition hover:bg-brand-hover disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint"
                type="submit"
                disabled={!canCreateMember}
              >
                <Plus className="h-4 w-4" aria-hidden="true" />
                <span>Add member</span>
              </button>
            </form>
            <div className="overflow-x-auto rounded-md border border-line">
              <table className="w-full min-w-[780px] table-fixed border-collapse text-left text-sm">
                <thead className="bg-rail text-xs uppercase text-faint">
                  <tr>
                    <th className="px-3 py-3 font-semibold">Member</th>
                    <th className="px-3 py-3 font-semibold">Role</th>
                    <th className="px-3 py-3 font-semibold">Verified</th>
                    <th className="px-3 py-3 font-semibold">Joined</th>
                    <th className="px-3 py-3 font-semibold" aria-label="Actions" />
                  </tr>
                </thead>
                <tbody className="divide-y divide-line">
                  {memberships.map((membership) => {
                    const lastOwner = isLastOwner(memberships, membership.id);
                    return (
                      <tr key={membership.id} className="bg-elevated/35">
                        <td className="px-3 py-3">
                          <p className="truncate font-medium text-ink">{membership.display_name}</p>
                          <p className="truncate text-xs text-faint">{membership.email}</p>
                        </td>
                        <td className="px-3 py-3">
                          <select
                            className="h-9 w-full rounded-md border border-line bg-panel px-2 text-sm text-ink disabled:cursor-not-allowed disabled:text-faint"
                            value={membership.role}
                            disabled={busy || lastOwner || !canManageMembers(currentOrgRole)}
                            onChange={(event) => changeMembershipRole(membership, event.target.value as MemberRole)}
                          >
                            {memberRoles.map((role) => (
                              <option
                                key={role.id}
                                value={role.id}
                                disabled={!canChangeMemberRole(currentOrgRole, membership.role, role.id)}
                              >
                                {role.label}
                              </option>
                            ))}
                          </select>
                          {lastOwner ? <p className="mt-1 text-xs text-faint">Last owner</p> : null}
                        </td>
                        <td className="px-3 py-3 text-muted">{membership.email_verified ? "Yes" : "No"}</td>
                        <td className="px-3 py-3 text-muted">{formatDateTime(membership.created_at)}</td>
                        <td className="px-3 py-3 text-right">
                          <button
                            className="inline-grid h-8 w-8 place-items-center rounded-md text-muted transition hover:bg-danger hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
                            type="button"
                            aria-label={`Remove ${membership.email}`}
                            disabled={busy || lastOwner || !canRemoveMember(currentOrgRole, membership.role)}
                            onClick={() => removeMembership(membership)}
                          >
                            <Trash2 className="h-4 w-4" aria-hidden="true" />
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                  {memberships.length === 0 ? (
                    <tr>
                      <td className="px-3 py-8 text-center text-sm text-muted" colSpan={5}>
                        No visible members for this organization.
                      </td>
                    </tr>
                  ) : null}
                </tbody>
              </table>
            </div>
          </section>
        );
      case "projects":
        return (
          <section className="space-y-4" aria-label="Project management">
            <ViewHeading icon={FolderKanban} title="Projects" description="Fleet, telemetry, action, and alert isolation." />
            <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
              <div className="space-y-2">
                {projects.map((project) => (
                  <ResourceRow
                    key={project.id}
                    active={project.id === currentProject?.id}
                    title={project.name}
                    subtitle={`/${project.slug}`}
                    meta={project.id}
                    activeLabel="Active"
                    actionLabel={project.id === selectedProject?.id ? "Open" : "Details"}
                    onOpen={() => selectProjectForDetail(project)}
                    onActivate={project.id === currentProject?.id ? undefined : () => onSelectProject(project.id)}
                    busy={busy}
                  />
                ))}
                {projects.length === 0 ? (
                  <div className="rounded-md border border-line bg-elevated px-3 py-8 text-center text-sm text-muted">
                    No projects in this organization.
                  </div>
                ) : null}
              </div>

              <div className="space-y-4">
                <form className="rounded-md border border-line bg-rail p-3" onSubmit={submitProject}>
                  <p className="text-sm font-semibold text-ink">Create project</p>
                  <div className="mt-3 flex flex-col gap-2 sm:flex-row xl:flex-col">
                    <input
                      className="h-10 min-w-0 flex-1 rounded-md border border-line bg-elevated px-3 text-sm text-ink placeholder:text-faint transition hover:border-faint"
                      value={projectName}
                      onChange={(event) => setProjectName(event.target.value)}
                      placeholder="New project"
                      type="text"
                    />
                    <button
                      className="inline-flex h-10 items-center justify-center gap-2 rounded-md bg-brand px-3 text-sm font-medium text-ink transition hover:bg-brand-hover disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint"
                      type="submit"
                      disabled={busy || !projectName.trim() || !canEditProject(currentOrgRole)}
                    >
                      <Plus className="h-4 w-4" aria-hidden="true" />
                      <span>Create</span>
                    </button>
                  </div>
                  <p className="mt-2 truncate text-xs text-faint">Slug preview: /{projectSlugPreview}</p>
                </form>

                {selectedProject ? (
                  <form className="rounded-md border border-line bg-elevated p-3" onSubmit={submitProjectUpdate}>
                    <div className="flex items-center justify-between gap-3">
                      <p className="truncate text-sm font-semibold text-ink">{selectedProject.name}</p>
                      {selectedProject.id !== currentProject?.id ? (
                        <button
                          className="h-8 rounded-md border border-line px-2 text-xs text-muted transition hover:bg-line hover:text-ink"
                          type="button"
                          disabled={busy}
                          onClick={() => onSelectProject(selectedProject.id)}
                        >
                          Set active
                        </button>
                      ) : null}
                    </div>
                    <div className="mt-3 space-y-3">
                      <TextField label="Name" value={projectDraft.name} onChange={(value) => setProjectDraft((draft) => ({ ...draft, name: value }))} />
                      <TextField label="Slug" value={projectDraft.slug} onChange={(value) => setProjectDraft((draft) => ({ ...draft, slug: value }))} />
                      {slugError(projectDraft.slug) ? <p className="text-xs text-warning">{slugError(projectDraft.slug)}</p> : null}
                      {!canEditProject(currentOrgRole) ? (
                        <p className="text-xs text-faint">Admin role is required for project settings.</p>
                      ) : null}
                      <SaveButton disabled={!canUpdateSelectedProject} />
                    </div>
                  </form>
                ) : (
                  <div className="rounded-md border border-line bg-elevated p-3 text-sm text-muted">
                    Create a project to enable project-scoped API keys and fleet operations.
                  </div>
                )}
              </div>
            </div>
          </section>
        );
      case "apiKeys":
        return (
          <section className="space-y-4" aria-label="API key management">
            <ViewHeading icon={KeyRound} title="API keys" description="Project-scoped automation credentials." />
            {!currentProject ? <InlineWarning message="Select or create a project before creating project-scoped API keys." /> : null}
            {apiKeyError ? <InlineWarning message={`Admin access is required for API key management: ${apiKeyError}`} /> : null}
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

            <div className="overflow-x-auto rounded-md border border-line">
              <table className="w-full min-w-[760px] table-fixed border-collapse text-left text-sm">
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
                        No API keys for this scope.
                      </td>
                    </tr>
                  ) : null}
                </tbody>
              </table>
            </div>
          </section>
        );
      case "audit":
        return (
          <section className="space-y-4" aria-label="Audit log">
            <ViewHeading icon={ScrollText} title="Audit log" description="Recent org and project scoped writes." />
            <div className="divide-y divide-line rounded-md border border-line">
              {audit.slice(0, 16).map((entry) => (
                <article
                  key={entry.id}
                  className="grid gap-2 bg-elevated/35 px-4 py-3 md:grid-cols-[220px_minmax(0,1fr)_220px] md:items-center"
                >
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
  }
}

function ViewHeading({
  icon: Icon,
  title,
  description,
}: {
  icon: typeof UserCircle;
  title: string;
  description: string;
}) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div>
        <h3 className="text-base font-semibold text-ink">{title}</h3>
        <p className="text-sm text-muted">{description}</p>
      </div>
      <Icon className="h-5 w-5 shrink-0 text-brand" aria-hidden="true" />
    </div>
  );
}

function TextField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="block text-sm font-medium text-muted">
      {label}
      <input
        className="mt-1 h-10 w-full rounded-md border border-line bg-panel px-3 text-sm text-ink transition hover:border-faint"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        type="text"
      />
    </label>
  );
}

function SaveButton({ disabled }: { disabled: boolean }) {
  return (
    <button
      className="inline-flex h-10 items-center justify-center gap-2 rounded-md bg-brand px-3 text-sm font-medium text-ink transition hover:bg-brand-hover disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint"
      type="submit"
      disabled={disabled}
    >
      <Save className="h-4 w-4" aria-hidden="true" />
      <span>Save</span>
    </button>
  );
}

function InlineWarning({ message }: { message: string }) {
  return (
    <div className="flex items-start gap-3 rounded-md border border-warning/30 bg-warning/10 p-3 text-sm text-warning">
      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
      <p>{message}</p>
    </div>
  );
}

function ResourceRow({
  active,
  title,
  subtitle,
  meta,
  activeLabel,
  actionLabel,
  onOpen,
  onActivate,
  busy,
}: {
  active: boolean;
  title: string;
  subtitle: string;
  meta: string;
  activeLabel: string;
  actionLabel: string;
  onOpen: () => void;
  onActivate?: () => void;
  busy: boolean;
}) {
  return (
    <article className={`rounded-md border px-3 py-3 ${active ? "border-brand/40 bg-brand/10" : "border-line bg-elevated"}`}>
      <div className="flex items-start justify-between gap-3">
        <button className="min-w-0 flex-1 text-left" type="button" onClick={onOpen}>
          <span className="block truncate text-sm font-semibold text-ink">{title}</span>
          <span className="mt-1 block truncate text-xs text-faint">{subtitle}</span>
          <span className="mt-2 block truncate text-xs text-faint">{meta}</span>
        </button>
        <div className="flex shrink-0 items-center gap-2">
          {active ? (
            <span className="rounded-sm bg-success/15 px-2 py-1 text-xs font-medium text-success">{activeLabel}</span>
          ) : null}
          <button
            className="h-8 rounded-md border border-line px-2 text-xs text-muted transition hover:bg-line hover:text-ink"
            type="button"
            onClick={onOpen}
          >
            {actionLabel}
          </button>
          {onActivate ? (
            <button
              className="h-8 rounded-md border border-line px-2 text-xs text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
              type="button"
              disabled={busy}
              onClick={onActivate}
            >
              Set active
            </button>
          ) : null}
        </div>
      </div>
    </article>
  );
}
