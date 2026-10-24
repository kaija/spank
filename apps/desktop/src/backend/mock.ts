import type { AppConfig } from "@/bindings/AppConfig";
import type { EventKind } from "@/bindings/EventKind";
import type { ImportOutcome } from "@/bindings/ImportOutcome";
import type { Profile } from "@/bindings/Profile";
import type { RuntimeState } from "@/bindings/RuntimeState";
import type { SoundEntry } from "@/bindings/SoundEntry";
import { EVENT_KINDS } from "@/lib/events";
import type { Backend, BackendEvents, FileDrop } from "./types";

/** In-memory backend for `pnpm dev` in a plain browser and for component tests. */
export function createMockBackend(seed: Partial<MockSeed> = {}): Backend & { emit: MockEmit; data: MockSeed } {
  const data: MockSeed = { ...defaultSeed(), ...structuredClone(seed) };
  const listeners = new Map<string, Set<(p: unknown) => void>>();
  const drops = new Set<(d: FileDrop) => void>();
  const emit: MockEmit = (event, payload) => listeners.get(event)?.forEach((cb) => cb(payload));
  const changed = (e: "profiles_changed" | "library_changed") => emit(e, null);
  let seq = 0;
  const find = (id: string) => {
    const p = data.profiles.find((x) => x.id === id);
    if (!p) throw new Error(`profile ${id} not found`);
    return p;
  };
  const writable = (id: string) => {
    const p = find(id);
    if (p.builtin) throw new Error(`profile ${id} is built in and read-only`);
    return p;
  };

  return {
    data,
    emit,
    async getState() {
      return structuredClone(data.state);
    },
    async getConfig() {
      return structuredClone(data.config);
    },
    async setMuteRules(rules) {
      data.config.mute = structuredClone(rules);
      return structuredClone(data.config);
    },
    async setPaused(paused) {
      data.config.paused = paused;
      data.state.paused = paused;
      emit("engine_state", structuredClone(data.state));
      return structuredClone(data.config);
    },
    async listProfiles() {
      return structuredClone(data.profiles);
    },
    async createProfile(name) {
      const p = emptyProfile(`user-${++seq}`, name.trim());
      data.profiles.push(p);
      changed("profiles_changed");
      return structuredClone(p);
    },
    async duplicateProfile(id, name) {
      const p = { ...structuredClone(find(id)), id: `user-${++seq}`, name: name.trim(), builtin: false };
      data.profiles.push(p);
      changed("profiles_changed");
      return structuredClone(p);
    },
    async saveProfile(profile) {
      const p = writable(profile.id);
      Object.assign(p, structuredClone(profile));
      changed("profiles_changed");
    },
    async renameProfile(id, name) {
      const p = writable(id);
      p.name = name.trim();
      changed("profiles_changed");
      return structuredClone(p);
    },
    async deleteProfile(id) {
      writable(id);
      data.profiles = data.profiles.filter((p) => p.id !== id);
      if (data.config.active_profile === id) {
        data.config.active_profile = "builtin.destruction";
        data.state.active_profile = "builtin.destruction";
      }
      changed("profiles_changed");
    },
    async setActiveProfile(id) {
      find(id);
      data.config.active_profile = id;
      data.state.active_profile = id;
      emit("engine_state", structuredClone(data.state));
      changed("profiles_changed");
    },
    async listSounds() {
      return structuredClone(data.sounds);
    },
    async soundPeaks(id) {
      const n = [...id].reduce((a, c) => a + c.charCodeAt(0), 0);
      return Array.from({ length: 32 }, (_, i) => Math.abs(Math.sin(i * 0.7 + n)) * Math.exp(-i / 14));
    },
    async importSound(path): Promise<ImportOutcome> {
      const file = path.split("/").pop() ?? path;
      const ext = file.split(".").pop()?.toLowerCase() ?? "";
      if (!["wav", "mp3", "m4a", "aac", "flac", "ogg"].includes(ext))
        return { status: "failed", reason: "unsupported", message: `unsupported or corrupt audio: .${ext}` };
      if (file.includes("huge")) return { status: "failed", reason: "too_large", message: "file is 12 MB" };
      const truncated = file.includes("long");
      const sound: SoundEntry = {
        id: `user-sound-${++seq}`,
        name: file.replace(/\.[^.]+$/, ""),
        source: { kind: "user", original_name: file },
        duration_ms: truncated ? 10_000 : 1500,
        sha256: "0".repeat(64),
        license: null,
        added_unix_ms: Date.now(),
      };
      data.sounds.push(sound);
      changed("library_changed");
      return { status: "added", sound, truncated, original_ms: truncated ? 23_400 : 1500 };
    },
    async renameSound(id, name) {
      const s = data.sounds.find((x) => x.id === id);
      if (!s || s.source.kind === "builtin") throw new Error("read-only");
      s.name = name.trim();
      changed("library_changed");
    },
    async deleteSound(id) {
      data.sounds = data.sounds.filter((s) => s.id !== id);
      const touched: string[] = [];
      for (const p of data.profiles.filter((x) => !x.builtin)) {
        for (const b of Object.values(p.mapping)) {
          if (b && b.sounds.includes(id)) {
            b.sounds = b.sounds.filter((s) => s !== id);
            if (!touched.includes(p.id)) touched.push(p.id);
          }
        }
      }
      changed("library_changed");
      changed("profiles_changed");
      return touched;
    },
    async previewSound(id) {
      data.log.push(`preview:${id}`);
    },
    async stopPreview() {
      data.log.push("stop");
    },
    async previewEvent(kind: EventKind) {
      data.log.push(`event:${kind}`);
    },
    async revealSound(id) {
      data.log.push(`reveal:${id}`);
    },
    async importPack(path) {
      data.log.push(`import-pack:${path}`);
      const p = emptyProfile(`user-${++seq}`, "Imported pack");
      data.profiles.push(p);
      changed("profiles_changed");
      return { sounds: 2, profile: p };
    },
    async exportPack(profileId, path) {
      data.log.push(`export:${profileId}:${path}`);
    },
    async requestFocusAuthorization() {
      data.state.focus_authorization = data.focusAnswer;
      emit("engine_state", structuredClone(data.state));
      return data.focusAnswer;
    },
    async openFocusSettings() {
      data.log.push("focus-settings");
    },
    async pickAudioFiles() {
      return data.pickFiles;
    },
    async pickPack() {
      return data.pickPack;
    },
    async pickPackSavePath(name) {
      return `/tmp/${name}`;
    },
    on<E extends keyof BackendEvents>(event: E, cb: (payload: BackendEvents[E]) => void) {
      const set = listeners.get(event) ?? new Set();
      set.add(cb as (p: unknown) => void);
      listeners.set(event, set);
      return () => set.delete(cb as (p: unknown) => void);
    },
    onFileDrop(cb) {
      drops.add(cb);
      data.dropFiles = (d: FileDrop) => drops.forEach((f) => f(d));
      return () => drops.delete(cb);
    },
  };
}

export type MockEmit = <E extends keyof BackendEvents>(event: E, payload: BackendEvents[E]) => void;

export type MockSeed = {
  state: RuntimeState;
  config: AppConfig;
  profiles: Profile[];
  sounds: SoundEntry[];
  /** What the fake file picker returns. */
  pickFiles: string[];
  pickPack: string | null;
  focusAnswer: RuntimeState["focus_authorization"];
  /** Calls that have no visible effect (previews, reveal…), for tests. */
  log: string[];
  /** Set by `onFileDrop`; tests call it to simulate dropping files on the window. */
  dropFiles?: (d: FileDrop) => void;
};

export function emptyProfile(id: string, name: string): Profile {
  return {
    id,
    name,
    builtin: false,
    volume: 1,
    mapping: Object.fromEntries(EVENT_KINDS.map((k) => [k, { enabled: true, sounds: [] }])),
  };
}

function builtinSound(stem: string, ms: number): SoundEntry {
  return {
    id: `builtin.${stem}`,
    name: stem,
    source: { kind: "builtin" },
    duration_ms: ms,
    sha256: "",
    license: "CC0-1.0",
    added_unix_ms: 0,
  };
}

function profile(id: string, name: string, builtin: boolean, map: Partial<Record<EventKind, string[]>>): Profile {
  const p = emptyProfile(id, name);
  p.builtin = builtin;
  for (const k of EVENT_KINDS) {
    const s = map[k];
    p.mapping[k] = { enabled: !!s?.length || !builtin, sounds: s ?? [] };
  }
  return p;
}

function defaultSeed(): MockSeed {
  return {
    state: {
      paused: false,
      mute: { reasons: [], focus_unavailable: false },
      active_profile: "user-office",
      lid_angle: 104,
      lid_running: true,
      accel_running: false,
      caps: { lid: { state: "available" }, accel: { state: "available" } },
      focus_authorization: "not_determined",
      audio_ok: true,
    },
    config: {
      schema_version: 1,
      active_profile: "user-office",
      app_volume: 0.8,
      paused: false,
      launch_at_login: true,
      language: "system",
      auto_check_updates: true,
      onboarding_done: false,
      lid: { open_min_deg: 10, close_threshold_deg: 20, overbend_threshold_deg: 120, hysteresis_deg: 5, cooldown_ms: 3000 },
      slap: {
        enabled: false,
        highpass_hz: 2,
        trigger_g: 0.15,
        refractory_ms: 150,
        levels: { light_g: 0.15, medium_g: 0.3, hard_g: 0.5 },
        combo_window_ms: 5000,
        combo_count: 3,
        precise_typing: false,
      },
      mute: {
        meeting: true,
        focus: true,
        follow_system: true,
        quiet_hours_enabled: true,
        quiet_hours: [
          { start: "12:30", end: "13:30", label: "午休" },
          { start: "23:00", end: "07:30", label: "睡覺" },
        ],
      },
    },
    profiles: [
      profile("builtin.classic", "經典", true, {
        lid_opened: ["builtin.creak"],
        lid_closing: ["builtin.thud-low"],
        lid_overbent: ["builtin.creak"],
        slap_light: ["builtin.thud-low"],
        slap_medium: ["builtin.thud-low"],
        slap_hard: ["builtin.thud-low"],
      }),
      profile("builtin.destruction", "毀滅", true, {
        lid_opened: ["builtin.glass-01", "builtin.glass-02"],
        lid_closing: ["builtin.thud-low"],
        lid_overbent: ["builtin.metal-snap", "builtin.metal-bend"],
        slap_light: ["builtin.glass-01"],
        slap_medium: ["builtin.glass-02"],
        slap_hard: ["builtin.metal-snap"],
        slap_combo: ["builtin.glass-crash"],
      }),
      profile("builtin.whine", "哀哀叫", true, {
        lid_opened: ["builtin.oh"],
        lid_closing: ["builtin.whimper"],
        lid_overbent: ["builtin.scream"],
        slap_light: ["builtin.hmm"],
        slap_medium: ["builtin.aiyo"],
        slap_hard: ["builtin.scream", "builtin.aiyo"],
        slap_combo: ["builtin.yelps"],
      }),
      profile("user-office", "辦公室安靜版", false, {
        lid_opened: ["builtin.oh"],
        lid_closing: ["builtin.whimper"],
        slap_hard: ["builtin.scream", "user-ouch"],
      }),
    ],
    sounds: [
      ...(
        [
          ["aiyo", 599],
          ["creak", 1499],
          ["glass-01", 800],
          ["glass-02", 1100],
          ["glass-crash", 1600],
          ["hmm", 449],
          ["metal-bend", 1576],
          ["metal-snap", 1400],
          ["oh", 399],
          ["scream", 1199],
          ["thud-low", 599],
          ["whimper", 899],
          ["yelps", 999],
        ] as const
      ).map(([s, ms]) => builtinSound(s, ms)),
      {
        id: "user-ouch",
        name: "好痛 02",
        source: { kind: "user", original_name: "好痛 02.m4a" },
        duration_ms: 700,
        sha256: "",
        license: null,
        added_unix_ms: 1,
      },
    ],
    pickFiles: [],
    pickPack: null,
    focusAnswer: "authorized",
    log: [],
  };
}
