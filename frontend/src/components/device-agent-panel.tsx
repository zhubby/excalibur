import { Download, FileText, KeyRound, PackageCheck, Terminal, Wrench } from "lucide-react";
import type { DeviceRow } from "@/lib/data";

const endpointRows = [
  {
    label: "CSR signing",
    path: "/api/v1/devices/{device_id}/provision/csr",
    tone: "text-teal",
    icon: KeyRound,
  },
  {
    label: "Dev auth JSON",
    path: "/api/v1/devices/{device_id}/provision/dev-auth",
    tone: "text-amber",
    icon: Download,
  },
];

export function DeviceAgentPanel({ device }: { device: DeviceRow }) {
  return (
    <section className="panel-in rounded-md border border-line bg-panel">
      <div className="flex flex-col gap-3 border-b border-line px-4 py-3 md:flex-row md:items-center md:justify-between">
        <div>
          <h2 className="text-base font-semibold text-ink">Device agent</h2>
          <p className="text-sm text-ink/54">{device.name} provisioning, OTA, diagnostics, and beta shell controls.</p>
        </div>
        <span className="inline-flex h-8 items-center gap-1.5 self-start rounded-sm bg-teal/10 px-2 text-xs font-medium text-teal md:self-auto">
          <PackageCheck className="h-3.5 w-3.5" aria-hidden="true" />
          native v1 protocol
        </span>
      </div>

      <div className="grid gap-4 p-4 lg:grid-cols-[minmax(0,1fr)_280px]">
        <div className="space-y-3">
          {endpointRows.map((row) => {
            const Icon = row.icon;
            return (
              <article key={row.label} className="rounded-md border border-line bg-white p-3">
                <div className="flex items-start gap-3">
                  <span className="grid h-8 w-8 shrink-0 place-items-center rounded-md bg-paper text-ink/70">
                    <Icon className={`h-4 w-4 ${row.tone}`} aria-hidden="true" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                      <p className="text-sm font-semibold text-ink">{row.label}</p>
                      <button className="inline-flex h-8 items-center justify-center gap-2 rounded-md border border-line bg-white px-2 text-xs font-medium text-ink transition hover:border-ink/20" type="button">
                        <Download className="h-3.5 w-3.5" aria-hidden="true" />
                        Download
                      </button>
                    </div>
                    <code className="mt-2 block break-all rounded-sm bg-paper px-2 py-1.5 text-xs text-ink/68">{row.path}</code>
                  </div>
                </div>
              </article>
            );
          })}
        </div>

        <div className="rounded-md border border-line bg-white p-3">
          <h3 className="text-sm font-semibold text-ink">Agent status</h3>
          <dl className="mt-3 space-y-2 text-xs">
            <div className="flex items-center justify-between gap-3">
              <dt className="text-ink/50">Version</dt>
              <dd className="font-medium text-ink">device-agent 2.18.5</dd>
            </div>
            <div className="flex items-center justify-between gap-3">
              <dt className="text-ink/50">Certificate</dt>
              <dd className="font-medium text-teal">active</dd>
            </div>
            <div className="flex items-center justify-between gap-3">
              <dt className="text-ink/50">Remote shell</dt>
              <dd className="font-medium text-amber">beta off</dd>
            </div>
          </dl>

          <div className="mt-4 grid grid-cols-3 gap-2">
            <button className="grid h-10 place-items-center rounded-md border border-line bg-white text-ink/70 transition hover:border-ink/20 hover:text-ink" type="button" aria-label="Trigger OTA install">
              <PackageCheck className="h-4 w-4" aria-hidden="true" />
            </button>
            <button className="grid h-10 place-items-center rounded-md border border-line bg-white text-ink/70 transition hover:border-ink/20 hover:text-ink" type="button" aria-label="Collect diagnostics">
              <Wrench className="h-4 w-4" aria-hidden="true" />
            </button>
            <button className="grid h-10 place-items-center rounded-md border border-line bg-paper text-ink/32" type="button" aria-label="Remote shell disabled" disabled>
              <Terminal className="h-4 w-4" aria-hidden="true" />
            </button>
          </div>

          <div className="mt-3 flex items-center gap-2 rounded-sm bg-paper px-2 py-2 text-xs text-ink/58">
            <FileText className="h-3.5 w-3.5 shrink-0 text-teal" aria-hidden="true" />
            Audit required for cert revoke, OTA, diagnostics, and shell sessions.
          </div>
        </div>
      </div>
    </section>
  );
}
