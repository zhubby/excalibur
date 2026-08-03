"use client";

import { ProjectsPanel } from "@/components/workspace-management-panels";
import { useConsoleRuntime } from "@/components/console-runtime";

export default function ProjectsPage() {
  const { busy, handleCreateProject, handleSelectProject, projects, workspace } = useConsoleRuntime();

  if (!workspace) {
    return (
      <section className="panel-in rounded-md border border-line bg-panel p-6 text-sm text-muted shadow-panel">
        Loading workspace...
      </section>
    );
  }

  return (
    <>
      <section className="flex flex-col gap-1">
        <p className="text-xs font-medium uppercase tracking-normal text-faint">Projects</p>
        <h2 className="text-xl font-semibold text-ink">Project switcher</h2>
        <p className="text-sm text-muted">Project-scoped fleet, stream, firmware, alert, and audit isolation.</p>
      </section>
      <ProjectsPanel
        currentProject={workspace.project}
        projects={projects}
        busy={busy}
        onCreateProject={handleCreateProject}
        onSelectProject={handleSelectProject}
      />
    </>
  );
}
