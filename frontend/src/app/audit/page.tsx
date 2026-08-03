"use client";

import { AuditLogPanel } from "@/components/workspace-management-panels";
import { useConsoleRuntime } from "@/components/console-runtime";

export default function AuditPage() {
  const { projectData, workspace } = useConsoleRuntime();

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
        <p className="text-xs font-medium uppercase tracking-normal text-faint">Audit</p>
        <h2 className="text-xl font-semibold text-ink">Control-plane writes</h2>
        <p className="text-sm text-muted">Recent org and project scoped operations.</p>
      </section>
      <AuditLogPanel audit={projectData.audit} />
    </>
  );
}
