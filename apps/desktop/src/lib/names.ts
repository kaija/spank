import type { Profile } from "@/bindings/Profile";
import type { SoundEntry } from "@/bindings/SoundEntry";

type T = (key: string, values?: Record<string, string | number>) => string;
type Has = (key: string) => boolean;

/** Built-in sounds and profiles are named in the message files; user ones use their stored name. */
export function soundName(t: T & { has: Has }, s: SoundEntry | undefined, fallbackId = ""): string {
  if (!s) return fallbackId.replace(/^builtin\./, "");
  if (s.source.kind === "builtin") {
    const key = `sounds.${s.id.replace(/^builtin\./, "")}`;
    if (t.has(key)) return t(key);
  }
  return s.name;
}

export function profileName(t: T & { has: Has }, p: Profile): string {
  if (p.builtin) {
    const key = `builtinProfiles.${p.id.replace(/^builtin\./, "")}.name`;
    if (t.has(key)) return t(key);
  }
  return p.name;
}

export function profileDesc(t: T & { has: Has }, p: Profile): string {
  const key = `builtinProfiles.${p.id.replace(/^builtin\./, "")}.desc`;
  return p.builtin && t.has(key) ? t(key) : "";
}
