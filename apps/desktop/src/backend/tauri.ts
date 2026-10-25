import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AUDIO_EXTENSIONS, type Backend, type BackendEvents, type FileDrop } from "./types";

/** Subscribe now, unsubscribe later, even if `listen` hasn't resolved yet. */
function subscribe(start: () => Promise<() => void>): () => void {
  let stop: (() => void) | null = null;
  let cancelled = false;
  void start().then((un) => {
    if (cancelled) un();
    else stop = un;
  });
  return () => {
    cancelled = true;
    stop?.();
  };
}

export const tauriBackend: Backend = {
  getState: () => invoke("get_state"),
  getConfig: () => invoke("get_config"),
  setMuteRules: (rules) => invoke("set_mute_rules", { rules }),
  setPaused: (paused) => invoke("set_paused", { paused }),
  appInfo: () => invoke("app_info"),

  setLidConfig: (lid) => invoke("set_lid_config", { lid }),
  setSlapConfig: (slap) => invoke("set_slap_config", { slap }),
  setSlapEnabled: (enabled) => invoke("set_slap_enabled", { enabled }),
  setAppVolume: (volume) => invoke("set_app_volume", { volume }),
  setLanguage: (language) => invoke("set_language", { language }),
  setLaunchAtLogin: (enabled) => invoke("set_launch_at_login", { enabled }),
  setAutoCheckUpdates: (enabled) => invoke("set_auto_check_updates", { enabled }),

  getCalibration: () => invoke("get_calibration"),
  openCalibration: () => invoke("open_calibration"),
  startCalibration: () => invoke("start_calibration"),
  calibrationStep: (step, reset) => invoke("calibration_step", { step, reset }),
  cancelCalibration: () => invoke("cancel_calibration"),
  finishCalibration: () => invoke("finish_calibration"),
  setAccelMonitor: (on) => invoke("set_accel_monitor", { on }),
  completeOnboarding: () => invoke("complete_onboarding"),

  requestInputMonitoring: () => invoke("request_input_monitoring"),
  openInputMonitoringSettings: () => invoke("open_input_monitoring_settings"),

  copyDiagnostics: () => invoke("copy_diagnostics"),
  checkForUpdates: () => invoke("check_for_updates"),
  installUpdate: () => invoke("install_update"),
  openLogFolder: () => invoke("open_log_folder"),

  listProfiles: () => invoke("list_profiles"),
  createProfile: (name) => invoke("create_profile", { name }),
  duplicateProfile: (id, name) => invoke("duplicate_profile", { id, name }),
  saveProfile: (profile) => invoke("save_profile", { profile }),
  renameProfile: (id, name) => invoke("rename_profile", { id, name }),
  deleteProfile: (id) => invoke("delete_profile", { id }),
  setActiveProfile: (id) => invoke("set_active_profile", { id }),

  listSounds: () => invoke("list_sounds"),
  soundPeaks: (id) => invoke("sound_peaks", { id }),
  importSound: (path) => invoke("import_sound", { path }),
  renameSound: (id, name) => invoke("rename_sound", { id, name }),
  deleteSound: (id) => invoke("delete_sound", { id }),
  previewSound: (id) => invoke("preview_sound", { id }),
  stopPreview: () => invoke("stop_preview"),
  previewEvent: (kind) => invoke("preview_event", { kind }),
  revealSound: (id) => invoke("reveal_sound", { id }),

  importPack: (path) => invoke("import_pack", { path }),
  exportPack: (profileId, path) => invoke("export_pack", { profileId, path }),

  requestFocusAuthorization: () => invoke("request_focus_authorization"),
  openFocusSettings: () => invoke("open_focus_settings"),

  async pickAudioFiles() {
    const r = await open({ multiple: true, filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }] });
    return r === null ? [] : Array.isArray(r) ? r : [r];
  },
  async pickPack() {
    const r = await open({ multiple: false, filters: [{ name: "Spank sound pack", extensions: ["spankpack"] }] });
    return typeof r === "string" ? r : null;
  },
  pickPackSavePath: (defaultName) =>
    save({ defaultPath: defaultName, filters: [{ name: "Spank sound pack", extensions: ["spankpack"] }] }),

  on<E extends keyof BackendEvents>(event: E, cb: (payload: BackendEvents[E]) => void) {
    return subscribe(() => listen<BackendEvents[E]>(event, (e) => cb(e.payload)));
  },

  onFileDrop(cb: (d: FileDrop) => void) {
    return subscribe(() =>
      getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "enter") cb({ type: "over", count: p.paths.length });
        else if (p.type === "drop") cb({ type: "drop", paths: p.paths });
        else if (p.type === "leave") cb({ type: "leave" });
      }),
    );
  },
};

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
