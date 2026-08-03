import { Download, FileText, KeyRound, PackageCheck, Terminal, Wrench } from "lucide-react";
import type { DeviceConfig } from "@/lib/api";
import type { DeviceRow } from "@/lib/data";
import { commandStatusTopic, commandTopic, shadowTopic, telemetryTopic } from "@/lib/protocol";

const endpointRows = [
  {
    label: "CSR signing",
    path: "/api/v1/devices/{device_id}/provision/csr",
    tone: "text-brand",
    icon: KeyRound,
  },
  {
    label: "Dev auth JSON",
    path: "/api/v1/devices/{device_id}/provision/dev-auth",
    tone: "text-warning",
    icon: Download,
  },
];

type DeviceAgentPanelProps = {
  device?: DeviceRow;
  projectId?: string;
  devAuthConfig?: DeviceConfig | null;
  busy?: boolean;
  onDownloadDevAuth: () => void;
  onIngestSample: () => void;
  onCreateDiagnostics: () => void;
  onCreateOta: () => void;
  onOpenRemoteShell: () => void;
  getRemoteShellDisabledReason: (deviceId?: string) => string | null;
};

export function DeviceAgentPanel({
  device,
  projectId,
  devAuthConfig,
  busy = false,
  onDownloadDevAuth,
  onIngestSample,
  onCreateDiagnostics,
  onCreateOta,
  onOpenRemoteShell,
  getRemoteShellDisabledReason,
}: DeviceAgentPanelProps) {
  const hasDevice = Boolean(device && projectId);
  const telemetry = hasDevice ? telemetryTopic(projectId!, device!.id, "device_agent_system_stats") : "-";
  const shadow = hasDevice ? shadowTopic(projectId!, device!.id) : "-";
  const commands = hasDevice ? commandTopic(projectId!, device!.id) : "-";
  const commandStatus = hasDevice ? commandStatusTopic(projectId!, device!.id) : "-";
  const authReady = devAuthConfig?.device_id === device?.id;
  const shellDisabledReason = getRemoteShellDisabledReason(device?.id);

  return (
    <section className="panel-in rounded-md border border-line bg-panel shadow-panel">
      <div className="flex flex-col gap-3 border-b border-line px-4 py-3 md:flex-row md:items-center md:justify-between">
        <div>
          <h2 className="text-base font-semibold text-ink">Device agent</h2>
          <p className="text-sm text-muted">
            {device ? `${device.name} provisioning, OTA, diagnostics, and beta shell controls.` : "Select a device to manage its agent."}
          </p>
        </div>
        <span className="inline-flex h-8 items-center gap-1.5 self-start rounded-sm bg-brand/15 px-2 text-xs font-medium text-brand md:self-auto">
          <PackageCheck className="h-3.5 w-3.5" aria-hidden="true" />
          native v1 protocol
        </span>
      </div>

      <div className="grid gap-4 p-4 lg:grid-cols-[minmax(0,1fr)_280px]">
        <div className="space-y-3">
          {endpointRows.map((row) => {
            const Icon = row.icon;
            return (
              <article key={row.label} className="rounded-md border border-line bg-elevated p-3">
                <div className="flex items-start gap-3">
                  <span className="grid h-8 w-8 shrink-0 place-items-center rounded-md bg-rail text-muted">
                    <Icon className={`h-4 w-4 ${row.tone}`} aria-hidden="true" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                      <p className="text-sm font-semibold text-ink">{row.label}</p>
                      <button
                        className="inline-flex h-8 items-center justify-center gap-2 rounded-md border border-line bg-panel px-2 text-xs font-medium text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
                        type="button"
                        disabled={busy || !hasDevice || row.label === "CSR signing"}
                        onClick={onDownloadDevAuth}
                      >
                        <Download className="h-3.5 w-3.5" aria-hidden="true" />
                        Download
                      </button>
                    </div>
                    <code className="mt-2 block break-all rounded-sm bg-rail px-2 py-1.5 text-xs text-muted">
                      {device ? row.path.replace("{device_id}", device.id) : row.path}
                    </code>
                  </div>
                </div>
              </article>
            );
          })}
          <article className="rounded-md border border-line bg-elevated p-3">
            <div className="grid gap-2 text-xs text-muted sm:grid-cols-2">
              <code className="break-all rounded-sm bg-rail px-2 py-1.5">{telemetry}</code>
              <code className="break-all rounded-sm bg-rail px-2 py-1.5">{shadow}</code>
              <code className="break-all rounded-sm bg-rail px-2 py-1.5">{commands}</code>
              <code className="break-all rounded-sm bg-rail px-2 py-1.5">{commandStatus}</code>
            </div>
          </article>
        </div>

        <div className="rounded-md border border-line bg-elevated p-3">
          <h3 className="text-sm font-semibold text-ink">Agent status</h3>
          <dl className="mt-3 space-y-2 text-xs">
            <div className="flex items-center justify-between gap-3">
              <dt className="text-faint">Version</dt>
              <dd className="font-medium text-ink">{device?.firmware ?? "-"}</dd>
            </div>
            <div className="flex items-center justify-between gap-3">
              <dt className="text-faint">Certificate</dt>
              <dd className={`font-medium ${authReady ? "text-success" : "text-warning"}`}>
                {authReady ? "dev auth ready" : "not issued"}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-3">
              <dt className="text-faint">Remote shell</dt>
              <dd className={`font-medium ${shellDisabledReason ? "text-warning" : "text-success"}`}>
                {shellDisabledReason ?? "ready"}
              </dd>
            </div>
          </dl>

          <div className="mt-4 grid grid-cols-3 gap-2">
            <button
              className="grid h-10 place-items-center rounded-md border border-line bg-panel text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
              type="button"
              aria-label="Trigger OTA install"
              disabled={busy || !hasDevice}
              onClick={onCreateOta}
            >
              <PackageCheck className="h-4 w-4" aria-hidden="true" />
            </button>
            <button
              className="grid h-10 place-items-center rounded-md border border-line bg-panel text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
              type="button"
              aria-label="Collect diagnostics"
              disabled={busy || !hasDevice}
              onClick={onCreateDiagnostics}
            >
              <Wrench className="h-4 w-4" aria-hidden="true" />
            </button>
            <button
              className="grid h-10 place-items-center rounded-md border border-line bg-panel text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:bg-rail disabled:text-faint"
              type="button"
              aria-label="Open remote shell"
              title={shellDisabledReason ?? "Open remote shell"}
              disabled={Boolean(shellDisabledReason)}
              onClick={onOpenRemoteShell}
            >
              <Terminal className="h-4 w-4" aria-hidden="true" />
            </button>
          </div>
          <button
            className="mt-2 inline-flex h-9 w-full items-center justify-center gap-2 rounded-md border border-line bg-panel text-xs font-medium text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
            type="button"
            disabled={busy || !hasDevice}
            onClick={onIngestSample}
          >
            <Wrench className="h-3.5 w-3.5" aria-hidden="true" />
            Simulate heartbeat
          </button>

          <div className="mt-3 flex items-center gap-2 rounded-sm bg-rail px-2 py-2 text-xs text-muted">
            <FileText className="h-3.5 w-3.5 shrink-0 text-brand" aria-hidden="true" />
            Audit required for cert revoke, OTA, diagnostics, and shell sessions.
          </div>
        </div>
      </div>
    </section>
  );
}
