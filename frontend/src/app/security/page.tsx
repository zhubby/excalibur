"use client";

import { AlertTriangle, Power, ScrollText, ShieldCheck, Terminal } from "lucide-react";
import { AlertPanel } from "@/components/action-alert-panels";
import { useConsoleRuntime } from "@/components/console-runtime";
import { formatDateTime } from "@/components/workspace-management-panels";

export default function SecurityPage() {
  const {
    alertSummaries,
    busy,
    handleCreateDefaultAlert,
    handleToggleRemoteShellFeature,
    projectData,
    protocolDeviceName,
    protocolTopics,
    remoteShellEnabled,
    remoteShellSessions,
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
        <p className="text-xs font-medium uppercase tracking-normal text-faint">Security</p>
        <h2 className="text-xl font-semibold text-ink">Protocol and audit posture</h2>
        <p className="text-sm text-muted">Device topics, alert rules, scoped writes, and RBAC-adjacent operational checks.</p>
      </section>

      <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_380px]">
        <section className="panel-in rounded-md border border-line bg-rail p-4 text-ink shadow-panel">
          <div className="flex items-start justify-between gap-3">
            <div>
              <h2 className="text-base font-semibold">Protocol topics</h2>
              <p className="mt-1 text-sm text-muted">{protocolDeviceName ? `${protocolDeviceName} topic bindings.` : "Select a device to inspect topic bindings."}</p>
            </div>
            <ShieldCheck className="h-5 w-5 text-success" aria-hidden="true" />
          </div>
          <div className="mt-4 space-y-3 text-xs text-muted">
            {protocolTopics.length > 0 ? (
              protocolTopics.map(([label, topic]) => (
                <div key={label}>
                  <p className="mb-1 text-faint">{label}</p>
                  <code className="block break-all rounded-sm bg-elevated p-2 text-ink">{topic}</code>
                </div>
              ))
            ) : (
              <p className="text-muted">No device selected.</p>
            )}
          </div>
        </section>

        <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
          <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
            <div>
              <h2 className="text-base font-semibold text-ink">Security summary</h2>
              <p className="text-sm text-muted">Current alert and write pressure.</p>
            </div>
            <AlertTriangle className="h-5 w-5 text-warning" aria-hidden="true" />
          </div>
          <div className="grid grid-cols-2 gap-3 p-4 text-sm">
            <article className="rounded-md border border-line bg-elevated p-3">
              <p className="text-xs text-faint">Alert rules</p>
              <p className="mt-3 text-2xl font-semibold text-ink">{alertSummaries.length}</p>
            </article>
            <article className="rounded-md border border-line bg-elevated p-3">
              <p className="text-xs text-faint">Firing</p>
              <p className="mt-3 text-2xl font-semibold text-ink">{alertSummaries.filter((alert) => alert.state === "firing").length}</p>
            </article>
          </div>
          <div className="divide-y divide-line border-t border-line">
            {projectData.audit.slice(0, 4).map((entry) => (
              <article key={entry.id} className="flex items-start gap-3 px-4 py-3">
                <ScrollText className="mt-0.5 h-4 w-4 shrink-0 text-brand" aria-hidden="true" />
                <div className="min-w-0">
                  <p className="truncate text-sm font-medium text-ink">{entry.action}</p>
                  <p className="truncate text-xs text-faint">{formatDateTime(entry.created_at)}</p>
                </div>
              </article>
            ))}
            {projectData.audit.length === 0 ? <div className="px-4 py-6 text-center text-sm text-muted">No audit entries yet.</div> : null}
          </div>
        </section>
      </div>

      <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
        <div className="flex flex-col gap-3 border-b border-line px-4 py-3 md:flex-row md:items-center md:justify-between">
          <div className="flex items-start gap-3">
            <span className="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-brand/15 text-brand">
              <Terminal className="h-4 w-4" aria-hidden="true" />
            </span>
            <div>
              <h2 className="text-base font-semibold text-ink">Remote shell beta</h2>
              <p className="text-sm text-muted">Project-gated interactive PTY sessions with short TTL and audit metadata.</p>
            </div>
          </div>
          <button
            className={`inline-flex h-9 items-center justify-center gap-2 rounded-md px-3 text-sm font-medium transition disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint ${
              remoteShellEnabled ? "border border-line bg-elevated text-ink hover:bg-line" : "bg-brand text-ink hover:bg-brand-hover"
            }`}
            type="button"
            disabled={busy}
            onClick={() => handleToggleRemoteShellFeature(!remoteShellEnabled)}
          >
            <Power className="h-4 w-4" aria-hidden="true" />
            {remoteShellEnabled ? "Disable" : "Enable"}
          </button>
        </div>
        <div className="grid gap-3 p-4 text-sm sm:grid-cols-3">
          <article className="rounded-md border border-line bg-elevated p-3">
            <p className="text-xs text-faint">Project flag</p>
            <p className={`mt-3 text-lg font-semibold ${remoteShellEnabled ? "text-success" : "text-warning"}`}>
              {remoteShellEnabled ? "enabled" : "off"}
            </p>
          </article>
          <article className="rounded-md border border-line bg-elevated p-3">
            <p className="text-xs text-faint">Active sessions</p>
            <p className="mt-3 text-lg font-semibold text-ink">
              {remoteShellSessions.filter((session) => !session.closed_at && (session.state === "Opening" || session.state === "Active")).length}
            </p>
          </article>
          <article className="rounded-md border border-line bg-elevated p-3">
            <p className="text-xs text-faint">Audit mode</p>
            <p className="mt-3 text-lg font-semibold text-ink">metadata only</p>
          </article>
        </div>
      </section>

      <AlertPanel rules={alertSummaries} busy={busy} onCreateDefault={handleCreateDefaultAlert} />
    </>
  );
}
