import type { LucideIcon } from "lucide-react";
import {
  Activity,
  Building2,
  FolderKanban,
  Gauge,
  KeyRound,
  ScrollText,
  ShieldCheck,
  UploadCloud,
  UserCircle,
  Users,
  Zap,
} from "lucide-react";

export type PrimaryNavSectionId = "fleet" | "streams" | "actions" | "firmware" | "security";
export type ManagementSectionId = "account" | "organizations" | "members" | "projects" | "apiKeys" | "audit";
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
  { id: "fleet", label: "Fleet", icon: Gauge },
  { id: "streams", label: "Streams", icon: Activity },
  { id: "actions", label: "Actions", icon: Zap },
  { id: "firmware", label: "Firmware", icon: UploadCloud },
  { id: "security", label: "Security", icon: ShieldCheck },
] satisfies Array<{
  id: PrimaryNavSectionId;
  label: string;
  icon: LucideIcon;
}>;

export const managementNavItems = [
  { id: "account", label: "Account", icon: UserCircle },
  { id: "organizations", label: "Organizations", icon: Building2 },
  { id: "members", label: "Members", icon: Users },
  { id: "projects", label: "Projects", icon: FolderKanban },
  { id: "apiKeys", label: "API keys", icon: KeyRound },
  { id: "audit", label: "Audit log", icon: ScrollText },
] satisfies Array<{
  id: ManagementSectionId;
  label: string;
  icon: LucideIcon;
}>;
