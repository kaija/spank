import type { AppConfig } from "@/bindings/AppConfig";
import type { CalStep } from "@/bindings/CalStep";
import type { Calibration } from "@/bindings/Calibration";
import type { CalibrationProgress } from "@/bindings/CalibrationProgress";
import type { EventKind } from "@/bindings/EventKind";
import type { FiredEvent } from "@/bindings/FiredEvent";
import type { FocusAuthorization } from "@/bindings/FocusAuthorization";
import type { ImportOutcome } from "@/bindings/ImportOutcome";
import type { InputAccess } from "@/bindings/InputAccess";
import type { Language } from "@/bindings/Language";
import type { LidConfig } from "@/bindings/LidConfig";
import type { MuteRules } from "@/bindings/MuteRules";
import type { PackImported } from "@/bindings/PackImported";
import type { Profile } from "@/bindings/Profile";
import type { ProfileId } from "@/bindings/ProfileId";
import type { RuntimeState } from "@/bindings/RuntimeState";
import type { SlapConfig } from "@/bindings/SlapConfig";
import type { SoundEntry } from "@/bindings/SoundEntry";
import type { SoundId } from "@/bindings/SoundId";

/** Events the Rust side emits (see apps/desktop/src-tauri/src/commands.rs). */
export type BackendEvents = {
  engine_state: RuntimeState;
  trigger_fired: FiredEvent;
  profiles_changed: null;
  library_changed: null;
  config_changed: AppConfig;
  calibration: CalibrationProgress;
  /** Largest |a| (g, gravity removed) of the last ~50 ms, while monitoring or calibrating. */
  accel_level: number;
  /** Update download progress, 0–1. */
  update_progress: number;
};

/** `app_info` (commands.rs). `edition`: see `@/lib/edition`. */
export type AppInfo = { version: string; defaultConfig: AppConfig; edition: "direct" | "appstore"; productName: string };
/** `check_for_updates` (apps/desktop/src-tauri/src/update.rs). */
export type UpdateInfo = { current: string; available: { version: string; notes: string | null } | null };

export type FileDrop = { type: "over"; count: number } | { type: "drop"; paths: string[] } | { type: "leave" };

/** Everything the settings UI needs from the app. `tauriBackend` is the real one; `createMockBackend` serves browser previews and tests. */
export interface Backend {
  getState(): Promise<RuntimeState>;
  getConfig(): Promise<AppConfig>;
  setMuteRules(rules: MuteRules): Promise<AppConfig>;
  setPaused(paused: boolean): Promise<AppConfig>;
  appInfo(): Promise<AppInfo>;

  setLidConfig(lid: LidConfig): Promise<AppConfig>;
  setSlapConfig(slap: SlapConfig): Promise<AppConfig>;
  setSlapEnabled(enabled: boolean): Promise<AppConfig>;
  setAppVolume(volume: number): Promise<AppConfig>;
  setLanguage(language: Language): Promise<AppConfig>;
  setLaunchAtLogin(enabled: boolean): Promise<AppConfig>;
  setAutoCheckUpdates(enabled: boolean): Promise<AppConfig>;

  getCalibration(): Promise<Calibration | null>;
  /** Open the calibration wizard window. */
  openCalibration(): Promise<void>;
  startCalibration(): Promise<void>;
  calibrationStep(step: CalStep, reset: boolean): Promise<void>;
  cancelCalibration(): Promise<void>;
  /** Saves the calibration, applies its levels and turns slaps on. */
  finishCalibration(): Promise<AppConfig>;
  /** Live waveform (`accel_level` events) for the Sensors page. */
  setAccelMonitor(on: boolean): Promise<void>;
  /** Marks onboarding done and closes the wizard window. */
  completeOnboarding(): Promise<void>;

  requestInputMonitoring(): Promise<InputAccess>;
  openInputMonitoringSettings(): Promise<void>;

  /** Diagnostics text (copied to the clipboard by `copyDiagnostics`). */
  copyDiagnostics(): Promise<string>;
  checkForUpdates(): Promise<UpdateInfo>;
  /** Download, install and restart into the update found by `checkForUpdates`. */
  installUpdate(): Promise<void>;
  openLogFolder(): Promise<void>;

  listProfiles(): Promise<Profile[]>;
  createProfile(name: string): Promise<Profile>;
  duplicateProfile(id: ProfileId, name: string): Promise<Profile>;
  saveProfile(profile: Profile): Promise<void>;
  renameProfile(id: ProfileId, name: string): Promise<Profile>;
  deleteProfile(id: ProfileId): Promise<void>;
  setActiveProfile(id: ProfileId): Promise<void>;

  listSounds(): Promise<SoundEntry[]>;
  soundPeaks(id: SoundId): Promise<number[]>;
  importSound(path: string): Promise<ImportOutcome>;
  renameSound(id: SoundId, name: string): Promise<void>;
  deleteSound(id: SoundId): Promise<ProfileId[]>;
  previewSound(id: SoundId): Promise<void>;
  stopPreview(): Promise<void>;
  previewEvent(kind: EventKind): Promise<void>;
  revealSound(id: SoundId): Promise<void>;

  importPack(path: string): Promise<PackImported>;
  exportPack(profileId: ProfileId, path: string): Promise<void>;

  requestFocusAuthorization(): Promise<FocusAuthorization>;
  openFocusSettings(): Promise<void>;

  /** Native file pickers. Empty / null when cancelled. */
  pickAudioFiles(): Promise<string[]>;
  pickPack(): Promise<string | null>;
  pickPackSavePath(defaultName: string): Promise<string | null>;

  on<E extends keyof BackendEvents>(event: E, cb: (payload: BackendEvents[E]) => void): () => void;
  onFileDrop(cb: (d: FileDrop) => void): () => void;
}

export const AUDIO_EXTENSIONS = ["wav", "mp3", "m4a", "aac", "flac", "ogg"];
