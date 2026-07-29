import { Boxes, Settings } from "lucide-react";
import { navItems } from "@/lib/data";

export function Sidebar() {
  return (
    <>
      <aside className="hidden min-h-screen w-64 shrink-0 bg-ink text-paper shadow-rail lg:flex lg:flex-col">
        <div className="border-b border-white/10 px-5 py-5">
          <div className="flex items-center gap-3">
            <div className="grid h-9 w-9 place-items-center rounded-md bg-teal text-white">
              <Boxes className="h-5 w-5" aria-hidden="true" />
            </div>
            <div>
              <p className="text-sm font-semibold leading-5">Excalibur</p>
              <p className="text-xs text-paper/60">IoT control plane</p>
            </div>
          </div>
        </div>

        <nav className="flex-1 space-y-1 px-3 py-4" aria-label="Main navigation">
          {navItems.map((item) => (
            <button
              key={item.label}
              className={`flex w-full items-center gap-3 rounded-md px-3 py-2.5 text-left text-sm transition ${
                item.active ? "bg-white text-ink" : "text-paper/72 hover:bg-white/10 hover:text-white"
              }`}
              type="button"
            >
              <item.icon className="h-4 w-4" aria-hidden="true" />
              <span>{item.label}</span>
            </button>
          ))}
        </nav>

        <div className="border-t border-white/10 p-3">
          <button
            className="flex w-full items-center gap-3 rounded-md px-3 py-2.5 text-left text-sm text-paper/72 transition hover:bg-white/10 hover:text-white"
            type="button"
          >
            <Settings className="h-4 w-4" aria-hidden="true" />
            <span>Project settings</span>
          </button>
        </div>
      </aside>

      <nav
        className="fixed inset-x-3 bottom-3 z-40 grid grid-cols-5 rounded-md border border-white/10 bg-ink p-1 text-paper shadow-lg lg:hidden"
        aria-label="Mobile navigation"
      >
        {navItems.map((item) => (
          <button
            key={item.label}
            className={`flex min-h-12 flex-col items-center justify-center gap-1 rounded-sm px-1 text-[11px] transition ${
              item.active ? "bg-white text-ink" : "text-paper/70 hover:bg-white/10 hover:text-white"
            }`}
            type="button"
          >
            <item.icon className="h-4 w-4" aria-hidden="true" />
            <span className="truncate">{item.label}</span>
          </button>
        ))}
      </nav>
    </>
  );
}
