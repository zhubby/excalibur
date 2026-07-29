import { AlertTriangle, CheckCircle2, Clock3, PackageCheck, PlayCircle, Wrench } from "lucide-react";
import type { ActionSummary, AlertSummary } from "@/lib/data";
import { clampProgress } from "@/lib/protocol";

const actionIcon = {
  queued: Clock3,
  running: PlayCircle,
  completed: CheckCircle2,
  failed: AlertTriangle,
  cancelled: AlertTriangle,
  "timed out": AlertTriangle,
  "waiting approval": Clock3,
} as const;

type ActionQueuePanelProps = {
  actions: ActionSummary[];
  busy?: boolean;
  canRunDeviceAction: boolean;
  onCreateDiagnostics: () => void;
  onCreateOta: () => void;
  onCompleteLatest: () => void;
};

export function ActionQueuePanel({
  actions,
  busy = false,
  canRunDeviceAction,
  onCreateDiagnostics,
  onCreateOta,
  onCompleteLatest,
}: ActionQueuePanelProps) {
  return (
    <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
      <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
        <div>
          <h2 className="text-base font-semibold text-ink">Actions</h2>
          <p className="text-sm text-muted">OTA, diagnostics, and command progress.</p>
        </div>
        <div className="flex gap-1">
          <button
            className="grid h-8 w-8 place-items-center rounded-md border border-line bg-elevated text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
            type="button"
            aria-label="Create diagnostics action"
            disabled={busy || !canRunDeviceAction}
            onClick={onCreateDiagnostics}
          >
            <Wrench className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
          <button
            className="grid h-8 w-8 place-items-center rounded-md border border-line bg-elevated text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
            type="button"
            aria-label="Create OTA action"
            disabled={busy || !canRunDeviceAction}
            onClick={onCreateOta}
          >
            <PackageCheck className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
          <button
            className="grid h-8 w-8 place-items-center rounded-md border border-line bg-elevated text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
            type="button"
            aria-label="Complete latest action"
            disabled={busy || actions.length === 0}
            onClick={onCompleteLatest}
          >
            <CheckCircle2 className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
        </div>
      </div>
      <div className="space-y-3 p-4">
        {actions.map((action) => {
          const Icon = actionIcon[action.state as keyof typeof actionIcon] ?? PlayCircle;
          const progress = clampProgress(action.progress);
          return (
            <article key={action.id} className="rounded-md border border-line bg-elevated p-3">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="truncate text-sm font-semibold text-ink">{action.name}</p>
                  <p className="truncate text-xs text-faint">{action.target}</p>
                </div>
                <Icon className="h-4 w-4 shrink-0 text-brand" aria-hidden="true" />
              </div>
              <div className="mt-3 h-2 rounded-full bg-rail">
                <div className="h-2 rounded-full bg-brand" style={{ width: `${progress}%` }} />
              </div>
              <div className="mt-2 flex justify-between text-xs text-muted">
                <span>{action.state}</span>
                <span>{progress}%</span>
              </div>
            </article>
          );
        })}
        {actions.length === 0 ? (
          <div className="rounded-md border border-line bg-elevated px-3 py-6 text-center text-sm text-muted">
            No actions queued.
          </div>
        ) : null}
      </div>
    </section>
  );
}

export function AlertPanel({
  rules,
  busy = false,
  onCreateDefault,
}: {
  rules: AlertSummary[];
  busy?: boolean;
  onCreateDefault: () => void;
}) {
  return (
    <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
      <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
        <div>
          <h2 className="text-base font-semibold text-ink">Alerts</h2>
          <p className="text-sm text-muted">Offline, threshold, and aggregate rules.</p>
        </div>
        <button
          className="grid h-8 w-8 place-items-center rounded-md border border-line bg-elevated text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
          type="button"
          aria-label="Create default alert"
          disabled={busy}
          onClick={onCreateDefault}
        >
          <AlertTriangle className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </div>
      <div className="divide-y divide-line">
        {rules.map((rule) => (
          <article key={rule.id} className="flex items-start gap-3 px-4 py-3">
            <span className={`mt-0.5 grid h-8 w-8 place-items-center rounded-md ${rule.state === "firing" ? "bg-danger/15 text-danger" : "bg-success/15 text-success"}`}>
              <AlertTriangle className="h-4 w-4" aria-hidden="true" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex items-center justify-between gap-3">
                <p className="truncate text-sm font-semibold text-ink">{rule.name}</p>
                <span className="text-xs text-faint">{rule.kind}</span>
              </div>
              <p className="truncate text-xs text-muted">{rule.target}</p>
            </div>
          </article>
        ))}
        {rules.length === 0 ? (
          <div className="px-4 py-6 text-center text-sm text-muted">No alert rules configured.</div>
        ) : null}
      </div>
    </section>
  );
}
