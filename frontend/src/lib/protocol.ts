export type TopicKind = "telemetry" | "shadow" | "commands" | "command-status";

function assertTopicSegment(name: string, value: string) {
  if (!value || /[\/+#]/.test(value)) {
    throw new Error(`${name} must be a non-empty MQTT topic segment without '/', '+', or '#'`);
  }
}

export function telemetryTopic(projectId: string, deviceId: string, stream: string) {
  assertTopicSegment("projectId", projectId);
  assertTopicSegment("deviceId", deviceId);
  assertTopicSegment("stream", stream);
  return `v1/p/${projectId}/d/${deviceId}/telemetry/${stream}`;
}

export function shadowTopic(projectId: string, deviceId: string) {
  assertTopicSegment("projectId", projectId);
  assertTopicSegment("deviceId", deviceId);
  return `v1/p/${projectId}/d/${deviceId}/shadow`;
}

export function commandTopic(projectId: string, deviceId: string) {
  assertTopicSegment("projectId", projectId);
  assertTopicSegment("deviceId", deviceId);
  return `v1/p/${projectId}/d/${deviceId}/commands`;
}

export function commandStatusTopic(projectId: string, deviceId: string) {
  assertTopicSegment("projectId", projectId);
  assertTopicSegment("deviceId", deviceId);
  return `v1/p/${projectId}/d/${deviceId}/commands/status`;
}

export function parseTopicKind(topic: string): TopicKind | null {
  const parts = topic.replace(/^\/|\/$/g, "").split("/");
  if (parts[0] !== "v1" || parts[1] !== "p" || parts[3] !== "d") {
    return null;
  }

  if (parts.length === 7 && parts[5] === "telemetry") {
    return "telemetry";
  }
  if (parts.length === 6 && parts[5] === "shadow") {
    return "shadow";
  }
  if (parts.length === 6 && parts[5] === "commands") {
    return "commands";
  }
  if (parts.length === 7 && parts[5] === "commands" && parts[6] === "status") {
    return "command-status";
  }
  return null;
}

export function clampProgress(value: number) {
  return Math.max(0, Math.min(100, Math.round(value)));
}
