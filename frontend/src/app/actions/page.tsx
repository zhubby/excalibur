"use client";

import { ActionQueuePanel } from "@/components/action-alert-panels";
import { useConsoleRuntime } from "@/components/console-runtime";

export default function ActionsPage() {
  const {
    allActionSummaries,
    busy,
    handleCompleteLatest,
    handleCreateDiagnostics,
    handleCreateOta,
    selectedDeviceId,
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
        <p className="text-xs font-medium uppercase tracking-normal text-faint">Actions</p>
        <h2 className="text-xl font-semibold text-ink">Action queue</h2>
        <p className="text-sm text-muted">OTA, diagnostics, command progress, and latest action completion.</p>
      </section>
      <div className="max-w-4xl">
        <ActionQueuePanel
          actions={allActionSummaries}
          busy={busy}
          canRunDeviceAction={Boolean(selectedDeviceId)}
          onCreateDiagnostics={handleCreateDiagnostics}
          onCreateOta={handleCreateOta}
          onCompleteLatest={handleCompleteLatest}
        />
      </div>
    </>
  );
}
