import {
  Activity,
  Bell,
  Gauge,
  RadioTower,
  ShieldCheck,
  UploadCloud,
  Wifi,
  Zap,
} from "lucide-react";

export type DeviceStatus = "online" | "offline" | "disabled" | "provisioned";

export type DeviceRow = {
  id: string;
  name: string;
  status: DeviceStatus;
  stream: string;
  firmware: string;
  lastSeen: string;
  rssi: number;
  shadow: string;
};

export const project = {
  org: "Northstar Mobility",
  name: "Factory EV Line",
  region: "us-east-1",
  projectId: "018f4c5c-9b4d-7cc2-a62a-44590f671001",
};

export const metrics = [
  {
    label: "Connected devices",
    value: "82,416",
    delta: "+2.8%",
    tone: "teal",
    icon: Wifi,
  },
  {
    label: "Telemetry ingest",
    value: "1.8M/min",
    delta: "p95 118ms",
    tone: "signal",
    icon: RadioTower,
  },
  {
    label: "Open actions",
    value: "247",
    delta: "31 waiting",
    tone: "amber",
    icon: Zap,
  },
  {
    label: "Alert pressure",
    value: "14",
    delta: "3 critical",
    tone: "danger",
    icon: Bell,
  },
];

export const devices: DeviceRow[] = [
  {
    id: "018f4c5c-9b4d-7cc2-a62a-44590f671101",
    name: "press-line-a-017",
    status: "online",
    stream: "device_agent_system_stats",
    firmware: "motor/v3.2.1",
    lastSeen: "8s ago",
    rssi: -58,
    shadow: "nominal",
  },
  {
    id: "018f4c5c-9b4d-7cc2-a62a-44590f671102",
    name: "weld-cell-b-044",
    status: "online",
    stream: "battery",
    firmware: "ecu/v4.9.0",
    lastSeen: "15s ago",
    rssi: -64,
    shadow: "charging",
  },
  {
    id: "018f4c5c-9b4d-7cc2-a62a-44590f671103",
    name: "qa-rig-c-006",
    status: "offline",
    stream: "device_shadow",
    firmware: "rootfs/v11.4.2",
    lastSeen: "22m ago",
    rssi: -92,
    shadow: "stale",
  },
  {
    id: "018f4c5c-9b4d-7cc2-a62a-44590f671104",
    name: "torque-arm-d-112",
    status: "provisioned",
    stream: "torque",
    firmware: "app/v1.18.0",
    lastSeen: "never",
    rssi: 0,
    shadow: "awaiting cert",
  },
];

export const telemetrySeries = [18, 22, 24, 21, 28, 31, 30, 34, 38, 35, 41, 44, 40, 47, 49, 46, 52, 56];

export const streamHealth = [
  { name: "device_shadow", rows: "82k", p95: "41ms", retention: "180d" },
  { name: "device_agent_system_stats", rows: "1.2B", p95: "96ms", retention: "90d" },
  { name: "battery", rows: "744M", p95: "103ms", retention: "180d" },
  { name: "commands/status", rows: "3.7M", p95: "37ms", retention: "365d" },
];

export const actionQueue = [
  { name: "ota.install", target: "3,120 devices", progress: 64, state: "running" },
  { name: "diagnostics.collect", target: "qa-rig-c-006", progress: 100, state: "completed" },
  { name: "remote_shell.open", target: "beta gated", progress: 0, state: "waiting approval" },
];

export const alertRules = [
  { name: "offline > 10m", kind: "offline", state: "firing", target: "44 devices" },
  { name: "battery temperature", kind: "threshold", state: "quiet", target: "battery stream" },
  { name: "ingest lag p95", kind: "window", state: "firing", target: "project aggregate" },
];

export const navItems = [
  { label: "Fleet", icon: Gauge, active: true },
  { label: "Streams", icon: Activity, active: false },
  { label: "Actions", icon: Zap, active: false },
  { label: "Firmware", icon: UploadCloud, active: false },
  { label: "Security", icon: ShieldCheck, active: false },
];
