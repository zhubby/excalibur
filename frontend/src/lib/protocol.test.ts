import { describe, expect, it } from "vitest";
import {
  clampProgress,
  commandStatusTopic,
  commandTopic,
  parseTopicKind,
  shadowTopic,
  telemetryTopic,
} from "./protocol";

describe("device protocol helpers", () => {
  const projectId = "018f4c5c-9b4d-7cc2-a62a-44590f671001";
  const deviceId = "018f4c5c-9b4d-7cc2-a62a-44590f671002";

  it("builds Excalibur native topics", () => {
    expect(telemetryTopic(projectId, deviceId, "battery")).toBe(
      `v1/p/${projectId}/d/${deviceId}/telemetry/battery`,
    );
    expect(shadowTopic(projectId, deviceId)).toBe(`v1/p/${projectId}/d/${deviceId}/shadow`);
    expect(commandTopic(projectId, deviceId)).toBe(`v1/p/${projectId}/d/${deviceId}/commands`);
    expect(commandStatusTopic(projectId, deviceId)).toBe(
      `v1/p/${projectId}/d/${deviceId}/commands/status`,
    );
  });

  it("classifies supported topic kinds", () => {
    expect(parseTopicKind(telemetryTopic(projectId, deviceId, "battery"))).toBe("telemetry");
    expect(parseTopicKind(commandStatusTopic(projectId, deviceId))).toBe("command-status");
    expect(parseTopicKind("v2/p/project/d/device/telemetry/battery")).toBeNull();
  });

  it("clamps action progress", () => {
    expect(clampProgress(-10)).toBe(0);
    expect(clampProgress(38.5)).toBe(39);
    expect(clampProgress(120)).toBe(100);
  });

  it("rejects invalid topic segments before building topics", () => {
    expect(() => telemetryTopic(projectId, deviceId, "")).toThrow("stream must be");
    expect(() => telemetryTopic(projectId, deviceId, "bad/stream")).toThrow("stream must be");
    expect(() => commandTopic(projectId, `${deviceId}#`)).toThrow("deviceId must be");
    expect(() => shadowTopic(`${projectId}+`, deviceId)).toThrow("projectId must be");
  });
});
