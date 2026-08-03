"use client";

import { Activity, CheckCircle2, PackageCheck, UploadCloud } from "lucide-react";
import { DeviceAgentPanel } from "@/components/device-agent-panel";
import { useConsoleRuntime } from "@/components/console-runtime";
import { formatDateTime } from "@/components/workspace-management-panels";
import { formatCount, humanizeEnum } from "@/lib/console-model";

function formatBytes(value: number) {
  if (value >= 1_073_741_824) {
    return `${(value / 1_073_741_824).toFixed(1)} GB`;
  }
  if (value >= 1_048_576) {
    return `${(value / 1_048_576).toFixed(1)} MB`;
  }
  if (value >= 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }
  return `${value} B`;
}

export default function FirmwarePage() {
  const {
    busy,
    dashboardStats,
    devAuthConfig,
    getRemoteShellDisabledReason,
    handleCreateDiagnostics,
    handleCreateOta,
    handleDownloadDevAuth,
    handleIngestSample,
    handleOpenRemoteShell,
    projectData,
    selectedDeviceRow,
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
        <p className="text-xs font-medium uppercase tracking-normal text-faint">Firmware</p>
        <h2 className="text-xl font-semibold text-ink">Artifacts and rollouts</h2>
        <p className="text-sm text-muted">Verified binaries, rollout state, device-agent provisioning, diagnostics, and OTA controls.</p>
      </section>

      <section className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4" aria-label="Firmware summary">
        {[
          ["Artifacts", formatCount(dashboardStats.firmwareCount), `${dashboardStats.activeFirmwareCount} active`, UploadCloud],
          ["Rollouts", formatCount(dashboardStats.rolloutCount), `${dashboardStats.activeRolloutCount} active`, Activity],
          ["Selected device", selectedDeviceRow?.name ?? "-", selectedDeviceRow?.firmware ?? "no firmware", PackageCheck],
          ["Verified", String(projectData.firmware.filter((artifact) => artifact.verified_at).length), "artifact checks", CheckCircle2],
        ].map(([label, value, delta, Icon]) => (
          <article key={label as string} className="panel-in rounded-md border border-line bg-panel p-4 shadow-panel">
            <div className="flex items-center justify-between gap-3">
              <p className="text-sm font-medium text-muted">{label as string}</p>
              <span className="grid h-9 w-9 place-items-center rounded-md bg-brand/15 text-brand">
                <Icon className="h-4 w-4" aria-hidden="true" />
              </span>
            </div>
            <div className="mt-4 flex items-end justify-between gap-3">
              <strong className="truncate text-2xl font-semibold text-ink">{value as string}</strong>
              <span className="text-xs font-medium text-faint">{delta as string}</span>
            </div>
          </article>
        ))}
      </section>

      <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_380px]">
        <section className="panel-in overflow-hidden rounded-md border border-line bg-panel shadow-panel">
          <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
            <div>
              <h2 className="text-base font-semibold text-ink">Firmware artifacts</h2>
              <p className="text-sm text-muted">Current project artifact inventory and verification state.</p>
            </div>
            <UploadCloud className="h-5 w-5 text-brand" aria-hidden="true" />
          </div>
          <div className="overflow-x-auto">
            <table className="min-w-[760px] w-full table-fixed border-collapse text-left text-sm">
              <thead className="bg-rail text-xs uppercase text-faint">
                <tr>
                  <th className="px-4 py-3 font-semibold">Version</th>
                  <th className="px-4 py-3 font-semibold">Component</th>
                  <th className="px-4 py-3 font-semibold">Size</th>
                  <th className="px-4 py-3 font-semibold">Verified</th>
                  <th className="px-4 py-3 font-semibold">Active</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-line">
                {projectData.firmware.map((artifact) => (
                  <tr key={artifact.id}>
                    <td className="px-4 py-3">
                      <p className="truncate font-medium text-ink">{artifact.version}</p>
                      <p className="truncate text-xs text-faint">{artifact.id}</p>
                    </td>
                    <td className="px-4 py-3 text-muted">{artifact.component}</td>
                    <td className="px-4 py-3 text-muted">{formatBytes(artifact.size_bytes)}</td>
                    <td className="px-4 py-3 text-muted">{formatDateTime(artifact.verified_at)}</td>
                    <td className="px-4 py-3">
                      <span className={`rounded-sm px-2 py-1 text-xs font-medium ${artifact.active ? "bg-success/15 text-success" : "bg-elevated text-faint"}`}>
                        {artifact.active ? "active" : "inactive"}
                      </span>
                    </td>
                  </tr>
                ))}
                {projectData.firmware.length === 0 ? (
                  <tr>
                    <td className="px-4 py-8 text-center text-sm text-muted" colSpan={5}>
                      No firmware artifacts yet.
                    </td>
                  </tr>
                ) : null}
              </tbody>
            </table>
          </div>
        </section>

        <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
          <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
            <div>
              <h2 className="text-base font-semibold text-ink">Rollouts</h2>
              <p className="text-sm text-muted">Recent firmware rollout cohorts.</p>
            </div>
            <Activity className="h-5 w-5 text-warning" aria-hidden="true" />
          </div>
          <div className="divide-y divide-line">
            {projectData.firmwareRollouts.slice(0, 8).map((rollout) => (
              <article key={rollout.id} className="px-4 py-3">
                <div className="flex items-center justify-between gap-3">
                  <p className="truncate text-sm font-semibold text-ink">{humanizeEnum(rollout.state)}</p>
                  <span className="text-xs text-faint">{rollout.cohort_size} devices</span>
                </div>
                <p className="mt-1 truncate text-xs text-muted">{rollout.strategy} · {formatDateTime(rollout.updated_at)}</p>
              </article>
            ))}
            {projectData.firmwareRollouts.length === 0 ? (
              <div className="px-4 py-8 text-center text-sm text-muted">No firmware rollouts yet.</div>
            ) : null}
          </div>
        </section>
      </div>

      <DeviceAgentPanel
        device={selectedDeviceRow}
        projectId={workspace.project.id}
        devAuthConfig={devAuthConfig}
        busy={busy}
        onDownloadDevAuth={() => handleDownloadDevAuth()}
        onIngestSample={() => handleIngestSample()}
        onCreateDiagnostics={handleCreateDiagnostics}
        onCreateOta={handleCreateOta}
        onOpenRemoteShell={() => handleOpenRemoteShell()}
        getRemoteShellDisabledReason={getRemoteShellDisabledReason}
      />
    </>
  );
}
