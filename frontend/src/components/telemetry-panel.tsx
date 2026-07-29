import { streamHealth, telemetrySeries } from "@/lib/data";

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
          <stop offset="0%" stopColor="#16786f" stopOpacity="0.24" />
          <stop offset="100%" stopColor="#16786f" stopOpacity="0.02" />
        </linearGradient>
      </defs>
      <polyline points={`0,${height} ${points} ${width},${height}`} fill="url(#telemetryFill)" stroke="none" />
      <polyline points={points} fill="none" stroke="#16786f" strokeLinecap="round" strokeLinejoin="round" strokeWidth="4" />
    </svg>
  );
}

export function TelemetryPanel() {
  return (
    <section className="panel-in rounded-md border border-line bg-panel">
      <div className="flex flex-col gap-1 border-b border-line px-4 py-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 className="text-base font-semibold text-ink">Telemetry</h2>
          <p className="text-sm text-ink/54">Timescale ingest and stream query health.</p>
        </div>
        <span className="text-sm font-medium text-teal">1.8M rows/min</span>
      </div>
      <div className="grid gap-4 p-4 xl:grid-cols-[1.5fr_1fr]">
        <div className="h-56 rounded-md border border-line bg-white p-3">
          <Sparkline values={telemetrySeries} />
        </div>
        <div className="space-y-2">
          {streamHealth.map((stream) => (
            <div key={stream.name} className="grid grid-cols-[1fr_auto] gap-3 rounded-md border border-line bg-white px-3 py-2">
              <div className="min-w-0">
                <p className="truncate text-sm font-medium text-ink">{stream.name}</p>
                <p className="text-xs text-ink/48">{stream.rows} rows</p>
              </div>
              <div className="text-right text-xs text-ink/58">
                <p>{stream.p95}</p>
                <p>{stream.retention}</p>
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
