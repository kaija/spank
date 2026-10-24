import type { AppConfig } from "@/bindings/AppConfig";
import type { EventKind } from "@/bindings/EventKind";
import type { FiredEvent } from "@/bindings/FiredEvent";
import type { FocusAuthorization } from "@/bindings/FocusAuthorization";
import type { ImportOutcome } from "@/bindings/ImportOutcome";
import type { MuteRules } from "@/bindings/MuteRules";
import type { PackImported } from "@/bindings/PackImported";
import type { Profile } from "@/bindings/Profile";
import type { ProfileId } from "@/bindings/ProfileId";
import type { RuntimeState } from "@/bindings/RuntimeState";
import type { SoundEntry } from "@/bindings/SoundEntry";
import type { SoundId } from "@/bindings/SoundId";

/** Events the Rust side emits (see apps/desktop/src-tauri/src/commands.rs). */
export type BackendEvents = {
  engine_state: RuntimeState;
  trigger_fired: FiredEvent;
  profiles_changed: null;
  library_changed: null;
};

export type FileDrop = { type: "over"; count: number } | { type: "drop"; paths: string[] } | { type: "leave" };

/** Everything the settings UI needs from the app. `tauriBackend` is the real one; `createMockBackend` serves browser previews and tests. */
export interface Backend {
  getState(): Promise<RuntimeState>;
  getConfig(): Promise<AppConfig>;
  setMuteRules(rules: MuteRules): Promise<AppConfig>;
  setPaused(paused: boolean): Promise<AppConfig>;

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
