"use client";

import { TelemetryPanel } from "@/components/telemetry-panel";
import { useConsoleRuntime } from "@/components/console-runtime";
import { formatCount } from "@/lib/console-model";

export default function StreamsPage() {
  const {
    busy,
    handleIngestSample,
    projectData,
    selectedDeviceRow,
    streamSummaries,
    telemetryValues,
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
        <p className="text-xs font-medium uppercase tracking-normal text-faint">Streams</p>
        <h2 className="text-xl font-semibold text-ink">Telemetry health</h2>
        <p className="text-sm text-muted">Timescale ingest trend, stream definitions, and selected-device sample ingest.</p>
      </section>
      <TelemetryPanel
        values={telemetryValues}
        streams={streamSummaries}
        rowRateLabel={`${formatCount(projectData.telemetry.length)} rows`}
        selectedDeviceName={selectedDeviceRow?.name}
        busy={busy}
        onIngestSample={() => handleIngestSample()}
      />
    </>
  );
}
