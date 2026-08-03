import { describe, expect, it } from "vitest";
import type { Action, AlertRule, Device, FirmwareArtifact, FirmwareRollout, StreamDefinition, TelemetryPoint } from "./api";
import {
  emptyProjectData,
  toActionSummaries,
  toAlertSummaries,
  toDashboardStats,
  toDeviceRows,
} from "./console-model";

const now = "2026-08-03T00:00:00.000Z";

function device(overrides: Partial<Device>): Device {
  return {
    created_at: now,
    id: "device-1",
    last_seen_at: now,
    latest_shadow: {},
    metadata: {},
    name: "linux-edge-001",
    project_id: "project-1",
    status: "Online",
    ...overrides,
  };
}

function stream(name: string): StreamDefinition {
  return {
    created_at: now,
    fields: [],
    id: `stream-${name}`,
    name,
    project_id: "project-1",
  };
}

function telemetry(overrides: Partial<TelemetryPoint>): TelemetryPoint {
  return {
    device_id: "device-1",
    ingested_at: now,
    payload: {},
    project_id: "project-1",
    sequence: 1,
    stream: "system",
    ts: now,
    ...overrides,
  };
}

function action(overrides: Partial<Action>): Action {
  return {
    created_at: now,
    device_ids: ["device-1"],
    errors: [],
    id: "action-1",
    name: "Diagnostics",
    payload: {},
    progress: 10,
    project_id: "project-1",
    state: "Running",
    updated_at: now,
    ...overrides,
  };
}

function alert(overrides: Partial<AlertRule>): AlertRule {
  return {
    enabled: true,
    expression: { window: "10m", stream: "system" },
    id: "alert-1",
    kind: "Offline",
    name: "offline > 10m",
    project_id: "project-1",
    ...overrides,
  };
}

function firmware(overrides: Partial<FirmwareArtifact>): FirmwareArtifact {
  return {
    active: false,
    component: "main",
    content_type: "application/octet-stream",
    created_at: now,
    id: "firmware-1",
    object_key: "firmware.bin",
    project_id: "project-1",
    sha256: "a".repeat(64),
    size_bytes: 1024,
    version: "1.0.0",
    ...overrides,
  };
}

function rollout(overrides: Partial<FirmwareRollout>): FirmwareRollout {
  return {
    action_id: "action-1",
    cohort_size: 1,
    created_at: now,
    firmware_id: "firmware-1",
    id: "rollout-1",
    project_id: "project-1",
    state: "Running",
    strategy: "all-at-once",
    updated_at: now,
    ...overrides,
  };
}

describe("console model helpers", () => {
  it("maps devices to table rows using telemetry, shadow, and metadata fallbacks", () => {
    const rows = toDeviceRows(
      [
        device({
          latest_shadow: {
            state: "nominal",
            firmware: { main: "main/1.2.3" },
          },
          metadata: { rssi_dbm: -66 },
        }),
      ],
      [stream("system")],
      [telemetry({ payload: { rssi_dbm: -55, cpu_percent: 42 } })],
    );

    expect(rows[0]).toMatchObject({
      id: "device-1",
      name: "linux-edge-001",
      status: "online",
      stream: "system",
      firmware: "main/1.2.3",
      rssi: -55,
      shadow: "nominal",
    });
  });

  it("limits action summaries only when a limit is supplied", () => {
    const actions = [
      action({ id: "a1", name: "First", progress: 10 }),
      action({ id: "a2", name: "Second", progress: 20 }),
      action({ id: "a3", name: "Third", progress: 30 }),
    ];
    const devices = [device({ id: "device-1", name: "Line 1" })];

    expect(toActionSummaries(actions, devices, 2).map((summary) => summary.id)).toEqual(["a1", "a2"]);
    expect(toActionSummaries(actions, devices).map((summary) => summary.id)).toEqual(["a1", "a2", "a3"]);
  });

  it("marks offline alerts as firing when any device is offline", () => {
    const summaries = toAlertSummaries(
      [alert({ kind: "Offline" }), alert({ id: "alert-2", kind: "Threshold", name: "cpu > 85" })],
      [device({ id: "device-1", status: "Offline" })],
    );

    expect(summaries).toMatchObject([
      { id: "alert-1", state: "firing", target: "system" },
      { id: "alert-2", state: "quiet" },
    ]);
  });

  it("builds dashboard stats from project data and derived rows", () => {
    const devices = [
      device({ id: "online", status: "Online" }),
      device({ id: "offline", status: "Offline" }),
      device({ id: "provisioned", status: "Provisioned" }),
    ];
    const projectData = {
      ...emptyProjectData,
      devices,
      streams: [stream("system")],
      telemetry: [telemetry({ device_id: "online" })],
      actions: [action({ state: "Running" }), action({ id: "done", state: "Completed" })],
      firmware: [firmware({ active: true })],
      firmwareRollouts: [rollout({ state: "Running" }), rollout({ id: "rollout-2", state: "Completed" })],
      alerts: [alert({ kind: "Offline" })],
    };
    const rows = toDeviceRows(projectData.devices, projectData.streams, projectData.telemetry);
    const alerts = toAlertSummaries(projectData.alerts, projectData.devices);

    expect(toDashboardStats(projectData, rows, alerts)).toMatchObject({
      totalDevices: 3,
      onlineDevices: 1,
      offlineDevices: 1,
      provisionedDevices: 1,
      onlinePercent: 33,
      telemetryRows: 1,
      openActions: 1,
      totalActions: 2,
      firingAlerts: 1,
      firmwareCount: 1,
      activeFirmwareCount: 1,
      rolloutCount: 2,
      activeRolloutCount: 1,
    });
  });
});
