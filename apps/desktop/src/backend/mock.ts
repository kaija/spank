import type { AppConfig } from "@/bindings/AppConfig";
import type { CalStep } from "@/bindings/CalStep";
import type { Calibration } from "@/bindings/Calibration";
import type { CalibrationProgress } from "@/bindings/CalibrationProgress";
import type { EventKind } from "@/bindings/EventKind";
import type { ImportOutcome } from "@/bindings/ImportOutcome";
import type { InputAccess } from "@/bindings/InputAccess";
import type { Profile } from "@/bindings/Profile";
import type { RuntimeState } from "@/bindings/RuntimeState";
import type { SoundEntry } from "@/bindings/SoundEntry";
import { EVENT_KINDS } from "@/lib/events";
import type { Backend, BackendEvents, FileDrop, UpdateInfo } from "./types";

/**
 * In-memory backend for `pnpm dev` in a plain browser and for component tests.
 * With `demo`, it also fakes the hardware: a live waveform while monitoring, and a slap shortly
 * after each calibration step starts (so the wizard can be walked through in a browser).
 */
export function createMockBackend(
  seed: Partial<MockSeed> = {},
  opts: { demo?: boolean } = {},
): Backend & { emit: MockEmit; data: MockSeed } {
  const data: MockSeed = { ...defaultSeed(), ...structuredClone(seed) };
  const listeners = new Map<string, Set<(p: unknown) => void>>();
  const drops = new Set<(d: FileDrop) => void>();
  const emit: MockEmit = (event, payload) => listeners.get(event)?.forEach((cb) => cb(payload));
  const changed = (e: "profiles_changed" | "library_changed") => emit(e, null);
  const config = () => {
    const c = structuredClone(data.config);
    emit("config_changed", structuredClone(c));
    return c;
  };
  const state = () => emit("engine_state", structuredClone(data.state));
  const progress = (p: CalibrationProgress) => {
    data.calProgress = p;
    emit("calibration", structuredClone(p));
  };
  let demoTimer: ReturnType<typeof setTimeout> | undefined;
  let waveTimer: ReturnType<typeof setInterval> | undefined;
  let monitoring = false;
  /** Demo waveform while the Sensors page watches (slaps on) or the wizard calibrates. */
  const syncWave = () => {
    clearInterval(waveTimer);
    if (!opts.demo || !(data.state.calibrating || (monitoring && data.config.slap.enabled))) return;
    let t = 0;
    waveTimer = setInterval(() => {
      t += 1;
      const hit = data.state.calibrating ? 0 : t % 61 === 0 ? 0.85 : t % 23 === 0 ? 0.4 : t % 13 === 0 ? 0.2 : 0;
      emit("accel_level", hit || 0.01 + Math.random() * 0.02);
    }, 50);
  };
  const demoSlap = (step: CalStep) => {
    clearTimeout(demoTimer);
    if (!opts.demo || !data.calProgress) return;
    if (step === "typing") {
      demoTimer = setTimeout(() => {
        const p = data.calProgress;
        if (p?.step === "typing") progress({ ...p, typing_g: 0.03, status: { status: "success", peak_g: 0.03 } });
      }, 5000);
      return;
    }
    const peak = { light: 0.18, medium: 0.42, hard: 0.95 }[step];
    demoTimer = setTimeout(() => {
      const p = data.calProgress;
      if (p?.step !== step) return;
      emit("accel_level", peak);
      progress({ ...p, [`${step}_g`]: peak, status: { status: "success", peak_g: peak } });
    }, 1800);
  };
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
      return config();
    },
    async setPaused(paused) {
      data.config.paused = paused;
      data.state.paused = paused;
      state();
      return config();
    },
    async appInfo() {
      return { version: "0.1.0", defaultConfig: defaultConfig() };
    },
    async setLidConfig(lid) {
      data.config.lid = structuredClone(lid);
      return config();
    },
    async setSlapConfig(slap) {
      data.config.slap = structuredClone(slap);
      data.state.accel_running = slap.enabled;
      state();
      syncWave();
      return config();
    },
    async setSlapEnabled(enabled) {
      data.config.slap.enabled = enabled;
      data.state.accel_running = enabled;
      state();
      syncWave();
      return config();
    },
    async setAppVolume(volume) {
      data.config.app_volume = Math.min(1, Math.max(0, volume));
      return config();
    },
    async setLanguage(language) {
      data.config.language = language;
      return config();
    },
    async setLaunchAtLogin(enabled) {
      data.config.launch_at_login = enabled;
      return config();
    },
    async setAutoCheckUpdates(enabled) {
      data.config.auto_check_updates = enabled;
      return config();
    },
    async getCalibration() {
      return structuredClone(data.calibration);
    },
    async openCalibration() {
      data.log.push("open-calibration");
    },
    async startCalibration() {
      data.state.calibrating = true;
      state();
      syncWave();
      progress({
        step: "light",
        status: { status: "waiting" },
        light_g: null,
        medium_g: null,
        hard_g: null,
        typing_g: null,
      });
      demoSlap("light");
    },
    async calibrationStep(step, reset) {
      data.log.push(`cal-step:${step}${reset ? ":reset" : ""}`);
      const p = data.calProgress;
      if (!p) return;
      const next = { ...p, step };
      if (step === "typing") {
        if (reset || p.typing_g === null) {
          next.typing_g = null;
          next.status = { status: "recording", progress: 0 };
        } else next.status = { status: "success", peak_g: p.typing_g };
      } else {
        const key = `${step}_g` as const;
        if (reset) next[key] = null;
        const v = next[key];
        next.status = v === null ? { status: "waiting" } : { status: "success", peak_g: v };
      }
      progress(next);
      if (next.status.status !== "success") demoSlap(step);
    },
    async cancelCalibration() {
      clearTimeout(demoTimer);
      data.calProgress = null;
      data.state.calibrating = false;
      state();
      syncWave();
    },
    async finishCalibration() {
      const p = data.calProgress;
      if (!p || p.light_g === null || p.medium_g === null || p.hard_g === null)
        throw new Error("calibration is missing a slap");
      const levels = {
        light_g: +(p.light_g * 0.6).toFixed(3),
        medium_g: +Math.sqrt(p.light_g * p.medium_g).toFixed(3),
        hard_g: +Math.sqrt(p.medium_g * p.hard_g).toFixed(3),
      };
      data.calibration = { levels, noise_floor_g: p.typing_g ?? 0, calibrated_unix_ms: Date.now() };
      data.config.slap = {
        ...data.config.slap,
        levels,
        trigger_g: Math.min(0.15, levels.light_g),
        enabled: true,
      };
      data.calProgress = null;
      data.state.calibrating = false;
      data.state.accel_running = true;
      state();
      syncWave();
      return config();
    },
    async setAccelMonitor(on) {
      data.log.push(`monitor:${on}`);
      monitoring = on;
      syncWave();
    },
    async completeOnboarding() {
      data.config.onboarding_done = true;
      data.log.push("onboarding-done");
      config();
    },
    async requestInputMonitoring() {
      data.state.input_access = data.inputAnswer;
      state();
      return data.inputAnswer;
    },
    async openInputMonitoringSettings() {
      data.log.push("input-settings");
    },
    async copyDiagnostics() {
      data.log.push("diagnostics");
      return "Spank 0.1.0\nmacOS 15.0\nMac: Mac16,12 (Apple M4)";
    },
    async openLogFolder() {
      data.log.push("log-folder");
    },
    async checkForUpdates() {
      if (data.update === "error") throw new Error("could not fetch a valid release JSON");
      return structuredClone(data.update);
    },
    async installUpdate() {
      if (data.update === "error" || !data.update.available) throw new Error("already up to date");
      for (const p of [0.25, 0.5, 1]) emit("update_progress", p);
      data.log.push(`install:${data.update.available.version}`);
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
      // Like the engine: a preview is reported as a fired event.
      const sound = data.profiles.find((p) => p.id === data.config.active_profile)?.mapping[kind]?.sounds[0];
      emit("trigger_fired", {
        event: { kind, at_ms: Date.now(), meta: { type: "none" } },
        result: sound ? { type: "played", sound } : { type: "no_sound" },
      });
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
  /** What `requestInputMonitoring` answers. */
  inputAnswer: InputAccess;
  calibration: Calibration | null;
  /** What `checkForUpdates` answers ("error" = it throws). */
  update: UpdateInfo | "error";
  /** The running calibration wizard, if any. */
  calProgress: CalibrationProgress | null;
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

/** `AppConfig::default()`. */
export function defaultConfig(): AppConfig {
  return {
    schema_version: 1,
    active_profile: "builtin.destruction",
    app_volume: 0.8,
    paused: false,
    launch_at_login: true,
    language: "system",
    auto_check_updates: true,
    onboarding_done: false,
    lid: { open_min_deg: 10, close_threshold_deg: 20, opening_start_deg: 30, overbend_threshold_deg: 120, hysteresis_deg: 5, cooldown_ms: 3000 },
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
    mute: { meeting: true, focus: true, follow_system: true, quiet_hours_enabled: true, quiet_hours: [] },
  };
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
      calibrating: false,
      input_access: "unknown",
      input_running: false,
    },
    config: {
      ...defaultConfig(),
      active_profile: "user-office",
      mute: {
        ...defaultConfig().mute,
        quiet_hours: [
          { start: "12:30", end: "13:30", label: "午休" },
          { start: "23:00", end: "07:30", label: "睡覺" },
        ],
      },
    },
    profiles: [
      profile("builtin.classic", "經典", true, {
        lid_opened: ["builtin.stone-door-01", "builtin.stone-door-02", "builtin.stone-door-03"],
        lid_closing: ["builtin.wood-click"],
        lid_overbent: ["builtin.grating-01", "builtin.grating-02", "builtin.grating-03", "builtin.wood-crack"],
        slap_light: ["builtin.thud-low"],
        slap_medium: ["builtin.thud-low"],
        slap_hard: ["builtin.thud-low"],
      }),
      profile("builtin.destruction", "毀滅", true, {
        lid_opened: ["builtin.glass-01", "builtin.glass-02"],
        lid_closing: ["builtin.thud-low"],
        lid_overbent: ["builtin.glass-tension-01", "builtin.glass-tension-02", "builtin.glass-tension-03"],
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
          ["glass-01", 800],
          ["grating-01", 3975],
          ["grating-02", 3909],
          ["grating-03", 3929],
          ["glass-02", 1100],
          ["glass-crash", 1600],
          ["glass-tension-01", 6943],
          ["glass-tension-02", 6946],
          ["glass-tension-03", 6984],
          ["hmm", 449],
          ["metal-snap", 1400],
          ["oh", 399],
          ["scream", 1199],
          ["stone-door-01", 4875],
          ["stone-door-02", 4997],
          ["stone-door-03", 5000],
          ["thud-low", 599],
          ["whimper", 899],
          ["wood-click", 1972],
          ["wood-crack", 3790],
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
    inputAnswer: "granted",
    calibration: null,
    calProgress: null,
    update: { current: "0.1.0", available: null },
    log: [],
  };
}
