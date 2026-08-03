import type { LucideIcon } from "lucide-react";
import { Activity, Building2, FolderKanban, Gauge, KeyRound, LayoutDashboard, ScrollText, ShieldCheck, UploadCloud, Zap } from "lucide-react";

export type PrimaryNavSectionId = "dashboard" | "fleet" | "streams" | "actions" | "firmware" | "security";
export type ManagementSectionId = "organization" | "projects" | "permissions" | "audit";
export type NavSectionId = PrimaryNavSectionId | ManagementSectionId;

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
  { id: "dashboard", label: "Dashboard", href: "/", icon: LayoutDashboard },
  { id: "fleet", label: "Fleet", href: "/fleet", icon: Gauge },
  { id: "streams", label: "Streams", href: "/streams", icon: Activity },
  { id: "actions", label: "Actions", href: "/actions", icon: Zap },
  { id: "firmware", label: "Firmware", href: "/firmware", icon: UploadCloud },
  { id: "security", label: "Security", href: "/security", icon: ShieldCheck },
] satisfies Array<{
  id: PrimaryNavSectionId;
  label: string;
  href: string;
  icon: LucideIcon;
}>;

export const managementNavItems = [
  { id: "organization", label: "Organization", href: "/organization", icon: Building2 },
  { id: "projects", label: "Projects", href: "/projects", icon: FolderKanban },
  { id: "permissions", label: "Permissions", href: "/permissions", icon: KeyRound },
  { id: "audit", label: "Audit log", href: "/audit", icon: ScrollText },
] satisfies Array<{
  id: ManagementSectionId;
  label: string;
  href: string;
  icon: LucideIcon;
}>;
