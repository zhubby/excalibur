"use client";

import { DeviceTable } from "@/components/device-table";
import { MetricStrip } from "@/components/metric-strip";
import { useConsoleRuntime } from "@/components/console-runtime";

export default function FleetPage() {
  const {
    busy,
    filteredDeviceRows,
    getRemoteShellDisabledReason,
    handleCreateDevice,
    handleDownloadDevAuth,
    handleIngestSample,
    handleOpenRemoteShell,
    metrics,
    selectedDeviceId,
    setSelectedDeviceId,
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
        <p className="text-xs font-medium uppercase tracking-normal text-faint">Fleet</p>
        <h2 className="text-xl font-semibold text-ink">Device operations</h2>
        <p className="text-sm text-muted">Provisioning, status, firmware, shadow state, and sample telemetry controls.</p>
      </section>
      <MetricStrip metrics={metrics} />
      <DeviceTable
        data={filteredDeviceRows}
        selectedDeviceId={selectedDeviceId}
        busy={busy}
        onCreateDevice={handleCreateDevice}
        onSelectDevice={setSelectedDeviceId}
        onDownloadDevAuth={handleDownloadDevAuth}
        onIngestSample={handleIngestSample}
        onOpenRemoteShell={handleOpenRemoteShell}
        getRemoteShellDisabledReason={getRemoteShellDisabledReason}
      />
    </>
  );
}
