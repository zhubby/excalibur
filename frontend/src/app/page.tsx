import { ActionQueuePanel, AlertPanel } from "@/components/action-alert-panels";
import { DeviceAgentPanel } from "@/components/device-agent-panel";
import { DeviceTable } from "@/components/device-table";
import { MetricStrip } from "@/components/metric-strip";
import { ProjectHeader } from "@/components/project-header";
import { Sidebar } from "@/components/sidebar";
import { TelemetryPanel } from "@/components/telemetry-panel";
import { devices, project } from "@/lib/data";
import { commandStatusTopic, commandTopic, shadowTopic, telemetryTopic } from "@/lib/protocol";

export default function Home() {
  const exampleTelemetryTopic = telemetryTopic(project.projectId, devices[0].id, "device_agent_system_stats");
  const exampleShadowTopic = shadowTopic(project.projectId, devices[0].id);
  const exampleCommandTopic = commandTopic(project.projectId, devices[0].id);
  const exampleStatusTopic = commandStatusTopic(project.projectId, devices[0].id);

  return (
    <main className="min-h-screen pb-20 lg:flex lg:pb-0">
      <Sidebar />
      <div className="min-w-0 flex-1">
        <ProjectHeader />
        <div className="space-y-5 px-4 py-5 md:px-6">
          <MetricStrip />

          <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
            <div className="space-y-5">
              <TelemetryPanel />
              <DeviceTable data={devices} />
              <DeviceAgentPanel device={devices[0]} />
            </div>

            <aside className="space-y-5">
              <ActionQueuePanel />
              <AlertPanel />
              <section className="panel-in rounded-md border border-line bg-ink p-4 text-paper">
                <h2 className="text-base font-semibold">Protocol</h2>
                <div className="mt-3 space-y-3 text-xs text-paper/68">
                  <div>
                    <p className="mb-1 text-paper/42">telemetry publish</p>
                    <code className="block break-all rounded-sm bg-white/8 p-2 text-paper">{exampleTelemetryTopic}</code>
                  </div>
                  <div>
                    <p className="mb-1 text-paper/42">shadow publish</p>
                    <code className="block break-all rounded-sm bg-white/8 p-2 text-paper">{exampleShadowTopic}</code>
                  </div>
                  <div>
                    <p className="mb-1 text-paper/42">commands subscribe</p>
                    <code className="block break-all rounded-sm bg-white/8 p-2 text-paper">{exampleCommandTopic}</code>
                  </div>
                  <div>
                    <p className="mb-1 text-paper/42">command status</p>
                    <code className="block break-all rounded-sm bg-white/8 p-2 text-paper">{exampleStatusTopic}</code>
                  </div>
                </div>
              </section>
            </aside>
          </div>
        </div>
      </div>
    </main>
  );
}
