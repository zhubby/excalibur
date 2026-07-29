import { Bell, ChevronDown, Search, ShieldCheck } from "lucide-react";
import { project } from "@/lib/data";

export function ProjectHeader() {
  return (
    <header className="border-b border-line bg-panel px-4 py-4 md:px-6">
      <div className="flex flex-col gap-4 xl:flex-row xl:items-center xl:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2 text-xs text-ink/58">
            <span>{project.org}</span>
            <span>/</span>
            <span>{project.region}</span>
            <span className="rounded-sm bg-teal/10 px-2 py-0.5 font-medium text-teal">tenant scoped</span>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-3">
            <h1 className="text-2xl font-semibold tracking-normal text-ink">{project.name}</h1>
            <button
              className="inline-flex h-8 items-center gap-1 rounded-md border border-line bg-white px-2 text-sm text-ink transition hover:border-ink/20"
              type="button"
            >
              <span>Project</span>
              <ChevronDown className="h-4 w-4" aria-hidden="true" />
            </button>
          </div>
        </div>

        <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
          <label className="relative block min-w-0 sm:w-80">
            <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/40" />
            <input
              className="h-10 w-full rounded-md border border-line bg-white pl-9 pr-3 text-sm text-ink placeholder:text-ink/38"
              placeholder="Search devices, streams, actions"
              aria-label="Search devices, streams, actions"
              type="search"
            />
          </label>
          <button
            className="inline-flex h-10 items-center justify-center gap-2 rounded-md border border-line bg-white px-3 text-sm text-ink transition hover:border-ink/20"
            type="button"
          >
            <ShieldCheck className="h-4 w-4 text-teal" aria-hidden="true" />
            <span>RBAC</span>
          </button>
          <button
            className="inline-flex h-10 w-10 items-center justify-center rounded-md bg-ink text-white transition hover:bg-ink/90"
            type="button"
            aria-label="Alerts"
          >
            <Bell className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
      </div>
    </header>
  );
}
