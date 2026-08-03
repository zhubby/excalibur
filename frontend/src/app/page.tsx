"use client";

import Link from "next/link";
import { Activity, AlertTriangle, CheckCircle2, Gauge, RadioTower, ScrollText, ShieldCheck, UploadCloud, Zap } from "lucide-react";
import { MetricStrip } from "@/components/metric-strip";
import { useConsoleRuntime } from "@/components/console-runtime";
import { formatDateTime } from "@/components/workspace-management-panels";
import { formatCount } from "@/lib/console-model";

function MiniSparkline({ values }: { values: number[] }) {
  const width = 520;
  const height = 150;
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
    <svg className="h-full w-full" viewBox={`0 0 ${width} ${height}`} role="img" aria-label="Telemetry trend">
      <polyline
        points={`0,${height} ${points} ${width},${height}`}
        fill="rgb(var(--color-brand) / 0.08)"
        stroke="none"
      />
      <polyline
        points={points}
        fill="none"
        stroke="rgb(var(--color-brand))"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="4"
      />
    </svg>
  );
}

const statusTone = {
  online: "bg-success/15 text-success",
  offline: "bg-danger/15 text-danger",
  provisioned: "bg-warning/15 text-warning",
  disabled: "bg-elevated text-faint",
} as const;

export default function DashboardPage() {
  const {
    actionSummaries,
    alertSummaries,
    dashboardStats,
    metrics,
    projectData,
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

  const statusRows = [
    ["online", dashboardStats.onlineDevices],
    ["offline", dashboardStats.offlineDevices],
    ["provisioned", dashboardStats.provisionedDevices],
    ["disabled", dashboardStats.disabledDevices],
  ] as const;
  const firingAlerts = alertSummaries.filter((alert) => alert.state === "firing");

  return (
    <>
      <section className="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
        <div>
          <p className="text-xs font-medium uppercase tracking-normal text-faint">Operations dashboard</p>
          <h2 className="mt-1 text-xl font-semibold text-ink">Project overview</h2>
          <p className="mt-1 text-sm text-muted">Fleet health, ingest pressure, rollout state, and recent writes.</p>
        </div>
        <div className="grid grid-cols-3 gap-2 text-xs text-muted sm:flex">
          <Link className="rounded-md border border-line bg-elevated px-3 py-2 transition hover:bg-line hover:text-ink" href="/fleet">
            Fleet
          </Link>
          <Link className="rounded-md border border-line bg-elevated px-3 py-2 transition hover:bg-line hover:text-ink" href="/streams">
            Streams
          </Link>
          <Link className="rounded-md border border-line bg-elevated px-3 py-2 transition hover:bg-line hover:text-ink" href="/actions">
            Actions
          </Link>
        </div>
      </section>

      <MetricStrip metrics={metrics} />

      <div className="grid gap-5 xl:grid-cols-[minmax(0,1.4fr)_minmax(320px,0.8fr)]">
        <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
          <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
            <div>
              <h2 className="text-base font-semibold text-ink">Telemetry trend</h2>
              <p className="text-sm text-muted">{formatCount(dashboardStats.telemetryRows)} rows across {streamSummaries.length} streams.</p>
            </div>
            <RadioTower className="h-5 w-5 text-brand" aria-hidden="true" />
          </div>
          <div className="grid gap-4 p-4 lg:grid-cols-[minmax(0,1fr)_260px]">
            <div className="h-60 rounded-md border border-line bg-elevated p-3">
              <MiniSparkline values={telemetryValues} />
            </div>
            <div className="space-y-2">
              {streamSummaries.slice(0, 5).map((stream) => (
                <article key={stream.name} className="rounded-md border border-line bg-elevated px-3 py-2">
                  <div className="flex items-center justify-between gap-3">
                    <p className="truncate text-sm font-medium text-ink">{stream.name}</p>
                    <span className="text-xs text-faint">{stream.retention}</span>
                  </div>
                  <p className="mt-1 text-xs text-muted">{stream.rows} rows</p>
                </article>
              ))}
              {streamSummaries.length === 0 ? (
                <div className="rounded-md border border-line bg-elevated px-3 py-6 text-center text-sm text-muted">
                  No stream definitions yet.
                </div>
              ) : null}
            </div>
          </div>
        </section>

        <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
          <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
            <div>
              <h2 className="text-base font-semibold text-ink">Device status</h2>
              <p className="text-sm text-muted">{dashboardStats.onlinePercent}% online in this project.</p>
            </div>
            <Gauge className="h-5 w-5 text-brand" aria-hidden="true" />
          </div>
          <div className="space-y-3 p-4">
            {statusRows.map(([status, count]) => {
              const width = dashboardStats.totalDevices === 0 ? 0 : Math.round((count / dashboardStats.totalDevices) * 100);
              return (
                <article key={status}>
                  <div className="flex items-center justify-between gap-3 text-sm">
                    <span className={`rounded-sm px-2 py-1 text-xs font-medium ${statusTone[status]}`}>{status}</span>
                    <span className="text-muted">{count}</span>
                  </div>
                  <div className="mt-2 h-2 rounded-full bg-rail">
                    <div className="h-2 rounded-full bg-brand" style={{ width: `${width}%` }} />
                  </div>
                </article>
              );
            })}
          </div>
        </section>
      </div>

      <div className="grid gap-5 xl:grid-cols-3">
        <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
          <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
            <div>
              <h2 className="text-base font-semibold text-ink">Recent actions</h2>
              <p className="text-sm text-muted">{dashboardStats.openActions} open operations.</p>
            </div>
            <Zap className="h-5 w-5 text-warning" aria-hidden="true" />
          </div>
          <div className="divide-y divide-line">
            {actionSummaries.slice(0, 4).map((action) => (
              <article key={action.id} className="px-4 py-3">
                <div className="flex items-center justify-between gap-3">
                  <p className="truncate text-sm font-semibold text-ink">{action.name}</p>
                  <span className="text-xs text-faint">{action.progress}%</span>
                </div>
                <p className="mt-1 truncate text-xs text-muted">{action.target} · {action.state}</p>
              </article>
            ))}
            {actionSummaries.length === 0 ? <div className="px-4 py-6 text-center text-sm text-muted">No actions queued.</div> : null}
          </div>
        </section>

        <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
          <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
            <div>
              <h2 className="text-base font-semibold text-ink">Alerts</h2>
              <p className="text-sm text-muted">{dashboardStats.firingAlerts} firing rules.</p>
            </div>
            <AlertTriangle className={`h-5 w-5 ${dashboardStats.firingAlerts > 0 ? "text-danger" : "text-success"}`} aria-hidden="true" />
          </div>
          <div className="divide-y divide-line">
            {(firingAlerts.length > 0 ? firingAlerts : alertSummaries).slice(0, 4).map((alert) => (
              <article key={alert.id} className="flex items-start gap-3 px-4 py-3">
                <span className={`mt-0.5 grid h-8 w-8 place-items-center rounded-md ${alert.state === "firing" ? "bg-danger/15 text-danger" : "bg-success/15 text-success"}`}>
                  <AlertTriangle className="h-4 w-4" aria-hidden="true" />
                </span>
                <div className="min-w-0">
                  <p className="truncate text-sm font-semibold text-ink">{alert.name}</p>
                  <p className="truncate text-xs text-muted">{alert.target}</p>
                </div>
              </article>
            ))}
            {alertSummaries.length === 0 ? <div className="px-4 py-6 text-center text-sm text-muted">No alert rules configured.</div> : null}
          </div>
        </section>

        <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
          <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
            <div>
              <h2 className="text-base font-semibold text-ink">Control plane</h2>
              <p className="text-sm text-muted">Firmware, rollouts, and recent audit.</p>
            </div>
            <ShieldCheck className="h-5 w-5 text-success" aria-hidden="true" />
          </div>
          <div className="grid grid-cols-2 gap-3 p-4 text-sm">
            <Link className="rounded-md border border-line bg-elevated p-3 transition hover:border-faint hover:text-ink" href="/firmware">
              <UploadCloud className="mb-3 h-4 w-4 text-brand" aria-hidden="true" />
              <p className="font-semibold text-ink">{dashboardStats.firmwareCount}</p>
              <p className="text-xs text-muted">Firmware artifacts</p>
            </Link>
            <Link className="rounded-md border border-line bg-elevated p-3 transition hover:border-faint hover:text-ink" href="/firmware">
              <Activity className="mb-3 h-4 w-4 text-warning" aria-hidden="true" />
              <p className="font-semibold text-ink">{dashboardStats.activeRolloutCount}</p>
              <p className="text-xs text-muted">Active rollouts</p>
            </Link>
          </div>
          <div className="divide-y divide-line border-t border-line">
            {projectData.audit.slice(0, 3).map((entry) => (
              <article key={entry.id} className="flex items-start gap-3 px-4 py-3">
                <ScrollText className="mt-0.5 h-4 w-4 shrink-0 text-brand" aria-hidden="true" />
                <div className="min-w-0">
                  <p className="truncate text-sm font-medium text-ink">{entry.action}</p>
                  <p className="truncate text-xs text-faint">{formatDateTime(entry.created_at)}</p>
                </div>
              </article>
            ))}
            {projectData.audit.length === 0 ? (
              <div className="px-4 py-6 text-center text-sm text-muted">
                <CheckCircle2 className="mx-auto mb-2 h-4 w-4 text-success" aria-hidden="true" />
                No audit entries yet.
              </div>
            ) : null}
          </div>
        </section>
      </div>
    </>
  );
}
