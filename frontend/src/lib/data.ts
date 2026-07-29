import type { LucideIcon } from "lucide-react";
import { Activity, Gauge, ShieldCheck, UploadCloud, Zap } from "lucide-react";

export type DeviceStatus = "online" | "offline" | "disabled" | "provisioned";

export type DeviceRow = {
  id: string;
  name: string;
  status: DeviceStatus;
  stream: string;
  firmware: string;
  lastSeen: string;
  rssi: number | null;
  shadow: string;
};

export type MetricTone = "teal" | "signal" | "amber" | "danger";

export type MetricItem = {
  label: string;
  value: string;
  delta: string;
  tone: MetricTone;
  icon: LucideIcon;
};

export type StreamSummary = {
  name: string;
  rows: string;
  p95: string;
  retention: string;
};

export type ActionSummary = {
  id: string;
  name: string;
  target: string;
  progress: number;
  state: string;
};

export type AlertSummary = {
  id: string;
  name: string;
  kind: string;
  state: "quiet" | "firing";
  target: string;
};

export const navItems = [
  { label: "Fleet", icon: Gauge, active: true },
  { label: "Streams", icon: Activity, active: false },
  { label: "Actions", icon: Zap, active: false },
  { label: "Firmware", icon: UploadCloud, active: false },
  { label: "Security", icon: ShieldCheck, active: false },
];
