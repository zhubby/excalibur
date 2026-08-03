"use client";

import { OrganizationPanel } from "@/components/workspace-management-panels";
import { useConsoleRuntime } from "@/components/console-runtime";

export default function OrganizationPage() {
  const { busy, handleCreateOrg, handleSelectOrg, orgs, workspace } = useConsoleRuntime();

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
        <p className="text-xs font-medium uppercase tracking-normal text-faint">Organization</p>
        <h2 className="text-xl font-semibold text-ink">Tenant boundary</h2>
        <p className="text-sm text-muted">Create, inspect, and switch accessible organizations.</p>
      </section>
      <OrganizationPanel
        currentOrg={workspace.org}
        orgs={orgs}
        busy={busy}
        onCreateOrg={handleCreateOrg}
        onSelectOrg={handleSelectOrg}
      />
    </>
  );
}
