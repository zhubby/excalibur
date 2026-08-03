"use client";

import type { ReactNode } from "react";
import { Boxes, SunMoon } from "lucide-react";
import { ConsoleRuntimeProvider, useConsoleRuntime } from "@/components/console-runtime";
import { ProjectHeader } from "@/components/project-header";
import { RemoteShellTerminal } from "@/components/remote-shell-terminal";
import { Sidebar } from "@/components/sidebar";

function ConsoleLogin() {
  const {
    apiBaseUrl,
    authMode,
    busy,
    displayName,
    email,
    error,
    handleAuthenticate,
    handleToggleTheme,
    password,
    setApiBaseUrl,
    setAuthMode,
    setDisplayName,
    setEmail,
    setPassword,
  } = useConsoleRuntime();

  return (
    <main className="grid min-h-screen place-items-center bg-paper px-4 py-10">
      <form className="w-full max-w-md rounded-md border border-line bg-panel p-5 shadow-panel" onSubmit={handleAuthenticate}>
        <div className="flex items-start justify-between gap-3">
          <div className="flex items-center gap-3">
            <div className="grid h-10 w-10 place-items-center rounded-md bg-brand text-ink">
              <Boxes className="h-5 w-5" aria-hidden="true" />
            </div>
            <div>
              <h1 className="text-lg font-semibold text-ink">Excalibur Console</h1>
              <p className="text-sm text-muted">Control plane sign-in</p>
            </div>
          </div>
          <button
            className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md border border-line bg-elevated text-muted transition hover:bg-line hover:text-ink"
            type="button"
            aria-label="Toggle theme"
            title="Toggle theme"
            onClick={handleToggleTheme}
          >
            <SunMoon className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>

        <div className="mt-5 grid grid-cols-2 gap-2 rounded-md bg-rail p-1">
          <button
            className={`h-9 rounded-sm text-sm font-medium transition ${authMode === "register" ? "bg-elevated text-ink" : "text-muted hover:text-ink"}`}
            type="button"
            onClick={() => setAuthMode("register")}
          >
            Register
          </button>
          <button
            className={`h-9 rounded-sm text-sm font-medium transition ${authMode === "login" ? "bg-elevated text-ink" : "text-muted hover:text-ink"}`}
            type="button"
            onClick={() => setAuthMode("login")}
          >
            Login
          </button>
        </div>

        <label className="mt-4 block text-sm font-medium text-muted">
          API base URL
          <input
            className="mt-1 h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
            value={apiBaseUrl}
            onChange={(event) => setApiBaseUrl(event.target.value)}
            type="url"
          />
        </label>
        <label className="mt-3 block text-sm font-medium text-muted">
          Email
          <input
            className="mt-1 h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            type="email"
            required
          />
        </label>
        {authMode === "register" ? (
          <label className="mt-3 block text-sm font-medium text-muted">
            Display name
            <input
              className="mt-1 h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
              type="text"
            />
          </label>
        ) : null}
        <label className="mt-3 block text-sm font-medium text-muted">
          Password
          <input
            className="mt-1 h-10 w-full rounded-md border border-line bg-elevated px-3 text-sm text-ink transition hover:border-faint"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            type="password"
            minLength={12}
            required
          />
        </label>

        {error ? <p className="mt-3 rounded-sm bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p> : null}

        <button
          className="mt-5 h-10 w-full rounded-md bg-brand text-sm font-semibold text-ink transition hover:bg-brand-hover disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint"
          type="submit"
          disabled={busy}
        >
          {busy ? "Working..." : authMode === "register" ? "Create account" : "Sign in"}
        </button>
      </form>
    </main>
  );
}

function ConsoleShell({ children }: { children: ReactNode }) {
  const {
    apiBaseUrl,
    activeRemoteShell,
    busy,
    error,
    handleBootstrapDemo,
    handleCloseRemoteShell,
    handleDismissRemoteShell,
    handleLogout,
    handleRefresh,
    handleToggleTheme,
    notice,
    search,
    session,
    setSearch,
    sidebarUserLabel,
    workspace,
  } = useConsoleRuntime();

  if (!session) {
    return <ConsoleLogin />;
  }

  return (
    <>
      <main className="min-h-screen bg-paper pb-20 text-ink lg:flex lg:pb-0">
        <Sidebar
          orgName={workspace?.org.name ?? "Loading org"}
          projectName={workspace?.project.name ?? "Loading project"}
          userLabel={sidebarUserLabel}
          onLogout={handleLogout}
        />
        <div className="min-w-0 flex-1">
          <ProjectHeader
            orgName={workspace?.org.name ?? "Loading org"}
            projectName={workspace?.project.name ?? "Loading project"}
            apiBaseUrl={apiBaseUrl}
            search={search}
            busy={busy}
            onSearch={setSearch}
            onToggleTheme={handleToggleTheme}
            onRefresh={handleRefresh}
            onBootstrapDemo={handleBootstrapDemo}
            onLogout={handleLogout}
          />
          <div className="space-y-5 px-4 py-5 md:px-6">
            {error || notice ? (
              <div
                className={`rounded-md border px-4 py-3 text-sm ${
                  error ? "border-danger/25 bg-danger/10 text-danger" : "border-success/25 bg-success/10 text-success"
                }`}
              >
                {error ?? notice}
              </div>
            ) : null}
            {children}
          </div>
        </div>
      </main>
      {activeRemoteShell ? (
        <RemoteShellTerminal
          terminal={activeRemoteShell}
          busy={busy}
          onCloseSession={handleCloseRemoteShell}
          onDismiss={handleDismissRemoteShell}
        />
      ) : null}
    </>
  );
}

export function ConsoleChrome({ children }: { children: ReactNode }) {
  return (
    <ConsoleRuntimeProvider>
      <ConsoleShell>{children}</ConsoleShell>
    </ConsoleRuntimeProvider>
  );
}
