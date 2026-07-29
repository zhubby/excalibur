import { AlertTriangle, CheckCircle2, Clock3, PlayCircle } from "lucide-react";
import { actionQueue, alertRules } from "@/lib/data";
import { clampProgress } from "@/lib/protocol";

const actionIcon = {
  running: PlayCircle,
  completed: CheckCircle2,
  "waiting approval": Clock3,
} as const;

export function ActionQueuePanel() {
  return (
    <section className="panel-in rounded-md border border-line bg-panel">
      <div className="border-b border-line px-4 py-3">
        <h2 className="text-base font-semibold text-ink">Actions</h2>
        <p className="text-sm text-ink/54">OTA, diagnostics, and command progress.</p>
      </div>
      <div className="space-y-3 p-4">
        {actionQueue.map((action) => {
          const Icon = actionIcon[action.state as keyof typeof actionIcon] ?? PlayCircle;
          const progress = clampProgress(action.progress);
          return (
            <article key={`${action.name}-${action.target}`} className="rounded-md border border-line bg-white p-3">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="truncate text-sm font-semibold text-ink">{action.name}</p>
                  <p className="truncate text-xs text-ink/50">{action.target}</p>
                </div>
                <Icon className="h-4 w-4 shrink-0 text-teal" aria-hidden="true" />
              </div>
              <div className="mt-3 h-2 rounded-full bg-paper">
                <div className="h-2 rounded-full bg-teal" style={{ width: `${progress}%` }} />
              </div>
              <div className="mt-2 flex justify-between text-xs text-ink/54">
                <span>{action.state}</span>
                <span>{progress}%</span>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}

export function AlertPanel() {
  return (
    <section className="panel-in rounded-md border border-line bg-panel">
      <div className="border-b border-line px-4 py-3">
        <h2 className="text-base font-semibold text-ink">Alerts</h2>
        <p className="text-sm text-ink/54">Offline, threshold, and aggregate rules.</p>
      </div>
      <div className="divide-y divide-line">
        {alertRules.map((rule) => (
          <article key={rule.name} className="flex items-start gap-3 px-4 py-3">
            <span className={`mt-0.5 grid h-8 w-8 place-items-center rounded-md ${rule.state === "firing" ? "bg-danger/10 text-danger" : "bg-teal/10 text-teal"}`}>
              <AlertTriangle className="h-4 w-4" aria-hidden="true" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex items-center justify-between gap-3">
                <p className="truncate text-sm font-semibold text-ink">{rule.name}</p>
                <span className="text-xs text-ink/46">{rule.kind}</span>
              </div>
              <p className="truncate text-xs text-ink/54">{rule.target}</p>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

