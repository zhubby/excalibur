"use client";

import { useEffect, useRef, useState } from "react";
import { Boxes, ChevronUp, LogOut, UserCircle } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { managementNavItems, navItems } from "@/lib/data";

type SidebarProps = {
  orgName: string;
  projectName: string;
  userLabel: string;
  onLogout: () => void;
};

function isActiveHref(pathname: string, href: string) {
  return href === "/" ? pathname === "/" : pathname === href || pathname.startsWith(`${href}/`);
}

export function Sidebar({ orgName, projectName, userLabel, onLogout }: SidebarProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const pathname = usePathname();
  const managementActive = managementNavItems.some((item) => isActiveHref(pathname, item.href));

  useEffect(() => {
    if (!menuOpen) {
      return;
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setMenuOpen(false);
      }
    };
    const handlePointerDown = (event: PointerEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    document.addEventListener("pointerdown", handlePointerDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("pointerdown", handlePointerDown);
    };
  }, [menuOpen]);

  return (
    <>
      <aside className="hidden h-screen w-64 shrink-0 bg-rail text-ink shadow-rail lg:sticky lg:top-0 lg:flex lg:flex-col">
        <div className="border-b border-line/70 px-5 py-5">
          <Link className="flex items-center gap-3 rounded-md transition hover:text-ink" href="/">
            <div className="grid h-9 w-9 place-items-center rounded-md bg-brand text-ink">
              <Boxes className="h-5 w-5" aria-hidden="true" />
            </div>
            <div>
              <p className="text-sm font-semibold leading-5">Excalibur</p>
              <p className="text-xs text-muted">IoT control plane</p>
            </div>
          </Link>
        </div>

        <nav className="flex-1 space-y-1 px-3 py-4" aria-label="Main navigation">
          {navItems.map((item) => (
            <Link
              key={item.id}
              className={`flex w-full items-center gap-3 rounded-md px-3 py-2.5 text-left text-sm transition ${
                isActiveHref(pathname, item.href) ? "bg-brand text-ink" : "text-muted hover:bg-elevated hover:text-ink"
              }`}
              href={item.href}
              aria-current={isActiveHref(pathname, item.href) ? "page" : undefined}
            >
              <item.icon className="h-4 w-4" aria-hidden="true" />
              <span>{item.label}</span>
            </Link>
          ))}
        </nav>

        <div className="relative border-t border-line/70 p-3" ref={menuRef}>
          <button
            className={`flex w-full items-center gap-3 rounded-md px-3 py-2.5 text-left text-sm transition ${
              managementActive || menuOpen ? "bg-elevated text-ink" : "text-muted hover:bg-elevated hover:text-ink"
            }`}
            type="button"
            aria-controls="user-operations-menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          >
            <UserCircle className="h-4 w-4" aria-hidden="true" />
            <span className="min-w-0 flex-1">
              <span className="block truncate">{userLabel}</span>
              <span className="block truncate text-xs text-faint">{projectName}</span>
            </span>
            <ChevronUp className={`h-4 w-4 shrink-0 transition ${menuOpen ? "rotate-180" : ""}`} aria-hidden="true" />
          </button>
          {menuOpen ? (
            <div
              id="user-operations-menu"
              className="absolute inset-x-3 bottom-full mb-2 overflow-hidden rounded-md border border-line bg-panel shadow-panel"
              aria-label="User operations"
            >
              <div className="border-b border-line px-3 py-2">
                <p className="truncate text-xs font-medium text-faint">{orgName}</p>
                <p className="truncate text-sm font-semibold text-ink">{projectName}</p>
              </div>
              <div className="p-1">
                {managementNavItems.map((item) => (
                  <Link
                    key={item.id}
                    className={`flex w-full items-center gap-3 rounded-sm px-3 py-2.5 text-left text-sm transition ${
                      isActiveHref(pathname, item.href) ? "bg-brand text-ink" : "text-muted hover:bg-elevated hover:text-ink"
                    }`}
                    href={item.href}
                    aria-current={isActiveHref(pathname, item.href) ? "page" : undefined}
                    onClick={() => setMenuOpen(false)}
                  >
                    <item.icon className="h-4 w-4" aria-hidden="true" />
                    <span>{item.label}</span>
                  </Link>
                ))}
              </div>
              <div className="border-t border-line p-1">
                <button
                  className="flex w-full items-center gap-3 rounded-sm px-3 py-2.5 text-left text-sm text-muted transition hover:bg-danger hover:text-ink"
                  type="button"
                  onClick={() => {
                    setMenuOpen(false);
                    onLogout();
                  }}
                >
                  <LogOut className="h-4 w-4" aria-hidden="true" />
                  <span>Sign out</span>
                </button>
              </div>
            </div>
          ) : null}
        </div>
      </aside>

      <nav
        className="fixed inset-x-3 bottom-3 z-40 overflow-x-auto rounded-md border border-line bg-rail p-1 text-ink shadow-lg lg:hidden"
        aria-label="Mobile navigation"
      >
        <div className="flex min-w-max gap-1">
          {navItems.map((item) => (
            <Link
              key={item.id}
              className={`flex min-h-12 w-20 flex-col items-center justify-center gap-1 rounded-sm px-1 text-[11px] transition ${
                isActiveHref(pathname, item.href) ? "bg-brand text-ink" : "text-muted hover:bg-elevated hover:text-ink"
              }`}
              href={item.href}
              aria-current={isActiveHref(pathname, item.href) ? "page" : undefined}
            >
              <item.icon className="h-4 w-4" aria-hidden="true" />
              <span className="truncate">{item.label}</span>
            </Link>
          ))}
        </div>
      </nav>
    </>
  );
}
