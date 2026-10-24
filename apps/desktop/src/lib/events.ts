import type { EventKind } from "@/bindings/EventKind";
import type { IconName } from "@/ui/iconPaths";

/** Event rows in the order of the mockup (Profiles page, tray preview menu). */
export const EVENT_KINDS: readonly EventKind[] = [
  "lid_opened",
  "lid_closing",
  "lid_overbent",
  "slap_light",
  "slap_medium",
  "slap_hard",
  "slap_combo",
];

export const EVENT_ICONS: Record<EventKind, IconName> = {
  lid_opened: "evOpen",
  lid_closing: "evClose",
  lid_overbent: "evOver",
  slap_light: "tapL",
  slap_medium: "tapM",
  slap_hard: "tapH",
  slap_combo: "combo",
};

/** `1499` → `"1.5s"`. */
export function formatDuration(ms: number): string {
  return `${(ms / 1000).toFixed(1)}s`;
}

/** Minutes since midnight for "HH:MM". */
export function toMinutes(t: string): number {
  const [h, m] = t.split(":").map(Number);
  return (h || 0) * 60 + (m || 0);
}

/** Length of a quiet-hours range in minutes (crossing midnight wraps; equal ends = 24 h). */
export function rangeMinutes(start: string, end: string): number {
  const d = (toMinutes(end) - toMinutes(start) + 1440) % 1440;
  return d === 0 ? 1440 : d;
}
