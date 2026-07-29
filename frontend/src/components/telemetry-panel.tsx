import { RadioTower } from "lucide-react";
import type { StreamSummary } from "@/lib/data";

function Sparkline({ values }: { values: number[] }) {
  const width = 560;
  const height = 170;
  const safeValues = values.length === 0 ? [0, 0] : values.length === 1 ? [values[0], values[0]] : values;
  const min = Math.min(...safeValues);
  const max = Math.max(...safeValues);
  const points = safeValues
    .map((value, index) => {
      const x = (index / (safeValues.length - 1)) * width;
      const y = height - ((value - min) / (max - min || 1)) * height;
      return `${x},${y}`;
    })
    .join(" ");

  return (
    <svg className="h-full w-full" viewBox={`0 0 ${width} ${height}`} role="img" aria-label="Telemetry ingest trend">
      <defs>
        <linearGradient id="telemetryFill" x1="0" x2="0" y1="0" y2="1">
          <stop offset="0%" stopColor="rgb(var(--color-brand))" stopOpacity="0.3" />
          <stop offset="100%" stopColor="rgb(var(--color-brand))" stopOpacity="0.03" />
        </linearGradient>
      </defs>
      <polyline points={`0,${height} ${points} ${width},${height}`} fill="url(#telemetryFill)" stroke="none" />
      <polyline points={points} fill="none" stroke="rgb(var(--color-brand))" strokeLinecap="round" strokeLinejoin="round" strokeWidth="4" />
    </svg>
  );
}

type TelemetryPanelProps = {
  values: number[];
  streams: StreamSummary[];
  rowRateLabel: string;
  selectedDeviceName?: string;
  busy?: boolean;
  onIngestSample: () => void;
};

export function TelemetryPanel({
  values,
  streams,
  rowRateLabel,
  selectedDeviceName,
  busy = false,
  onIngestSample,
}: TelemetryPanelProps) {
  return (
    <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
      <div className="flex flex-col gap-1 border-b border-line px-4 py-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 className="text-base font-semibold text-ink">Telemetry</h2>
          <p className="text-sm text-muted">
            {selectedDeviceName ? `${selectedDeviceName} stream query health.` : "Timescale ingest and stream query health."}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-brand">{rowRateLabel}</span>
          <button
            className="inline-flex h-8 items-center justify-center gap-2 rounded-md border border-line bg-elevated px-2 text-xs font-medium text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
            type="button"
            disabled={busy || !selectedDeviceName}
            onClick={onIngestSample}
          >
            <RadioTower className="h-3.5 w-3.5" aria-hidden="true" />
            Ingest sample
          </button>
        </div>
      </div>
      <div className="grid gap-4 p-4 xl:grid-cols-[1.5fr_1fr]">
        <div className="h-56 rounded-md border border-line bg-elevated p-3">
          <Sparkline values={values} />
        </div>
        <div className="space-y-2">
          {streams.map((stream) => (
            <div key={stream.name} className="grid grid-cols-[1fr_auto] gap-3 rounded-md border border-line bg-elevated px-3 py-2">
              <div className="min-w-0">
                <p className="truncate text-sm font-medium text-ink">{stream.name}</p>
                <p className="text-xs text-faint">{stream.rows} rows</p>
              </div>
              <div className="text-right text-xs text-muted">
                <p>{stream.p95}</p>
                <p>{stream.retention}</p>
              </div>
            </div>
          ))}
          {streams.length === 0 ? (
            <div className="rounded-md border border-line bg-elevated px-3 py-6 text-center text-sm text-muted">
              No stream definitions yet.
            </div>
          ) : null}
        </div>
      </div>
    </section>
  );
}
