import type { MetricItem, MetricTone } from "@/lib/data";

const toneClass = {
  teal: "bg-success/15 text-success",
  signal: "bg-brand/15 text-brand",
  amber: "bg-warning/15 text-warning",
  danger: "bg-danger/15 text-danger",
} satisfies Record<MetricTone, string>;

export function MetricStrip({ metrics }: { metrics: MetricItem[] }) {
  return (
    <section className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4" aria-label="Fleet summary">
      {metrics.map((metric) => (
        <article key={metric.label} className="panel-in rounded-md border border-line bg-panel p-4 shadow-panel">
          <div className="flex items-center justify-between gap-3">
            <p className="text-sm font-medium text-muted">{metric.label}</p>
            <span className={`grid h-9 w-9 place-items-center rounded-md ${toneClass[metric.tone]}`}>
              <metric.icon className="h-4 w-4" aria-hidden="true" />
            </span>
          </div>
          <div className="mt-4 flex items-end justify-between gap-3">
            <strong className="text-2xl font-semibold text-ink">{metric.value}</strong>
            <span className="text-xs font-medium text-faint">{metric.delta}</span>
          </div>
        </article>
      ))}
    </section>
  );
}
