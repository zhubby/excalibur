"use client";

import { PermissionsPanel } from "@/components/workspace-management-panels";
import { useConsoleRuntime } from "@/components/console-runtime";

export default function PermissionsPage() {
  const {
    apiKeyError,
    apiKeys,
    busy,
    createdApiKey,
    handleCreateApiKey,
    handleRevokeApiKey,
    workspace,
  } = useConsoleRuntime();

  if (!workspace) {
    return (
      <section className="panel-in rounded-md border border-line bg-panel p-6 text-sm text-muted shadow-panel">
        Loading workspace...
      </section>
    );
  }

  return (
    <>
      <section className="flex flex-col gap-1">
        <p className="text-xs font-medium uppercase tracking-normal text-faint">Permissions</p>
        <h2 className="text-xl font-semibold text-ink">API keys and RBAC</h2>
        <p className="text-sm text-muted">Project keys, scope presets, role reference, and member-management readiness.</p>
      </section>
      <PermissionsPanel
        apiKeys={apiKeys}
        apiKeyError={apiKeyError}
        createdApiKey={createdApiKey}
        busy={busy}
        onCreateApiKey={handleCreateApiKey}
        onRevokeApiKey={handleRevokeApiKey}
      />
    </>
  );
}
