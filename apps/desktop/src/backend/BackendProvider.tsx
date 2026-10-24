"use client";

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import type { AppConfig } from "@/bindings/AppConfig";
import type { Profile } from "@/bindings/Profile";
import type { RuntimeState } from "@/bindings/RuntimeState";
import type { SoundEntry } from "@/bindings/SoundEntry";
import { createMockBackend } from "./mock";
import { inTauri, tauriBackend } from "./tauri";
import type { Backend, BackendEvents } from "./types";

const BackendContext = createContext<Backend | null>(null);

/** Real backend inside the app, an in-memory one in a plain browser (`pnpm dev`) — or whatever a test passes. */
export function BackendProvider({ backend, children }: { backend?: Backend; children: ReactNode }) {
  const [value] = useState<Backend>(() => backend ?? (inTauri() ? tauriBackend : createMockBackend()));
  return <BackendContext.Provider value={value}>{children}</BackendContext.Provider>;
}

export function useBackend(): Backend {
  const b = useContext(BackendContext);
  if (!b) throw new Error("useBackend outside BackendProvider");
  return b;
}

/**
 * Load `fetch()` now and again whenever any of `events` fires.
 * Returns `[data, reload, setData]`; `data` is undefined until the first load finishes.
 */
export function useLive<T>(
  fetch: (b: Backend) => Promise<T>,
  events: readonly (keyof BackendEvents)[],
): [T | undefined, () => Promise<void>, (v: T) => void] {
  const backend = useBackend();
  const [data, setData] = useState<T>();
  const reload = useCallback(async () => {
    try {
      setData(await fetch(backend));
    } catch (e) {
      console.error(e);
    }
  }, [backend]);
  const key = events.join(",");
  useEffect(() => {
    void reload();
    const offs = events.map((e) => backend.on(e, () => void reload()));
    return () => offs.forEach((off) => off());
  }, [backend, reload, key]);
  return [data, reload, setData];
}

export function useRuntimeState(): RuntimeState | undefined {
  const backend = useBackend();
  const [state, setState] = useState<RuntimeState>();
  useEffect(() => {
    let alive = true;
    void backend.getState().then((s) => alive && setState(s));
    const off = backend.on("engine_state", (s) => setState(s));
    return () => {
      alive = false;
      off();
    };
  }, [backend]);
  return state;
}

export const useProfiles = () => useLive<Profile[]>((b) => b.listProfiles(), ["profiles_changed"]);
export const useSounds = () => useLive<SoundEntry[]>((b) => b.listSounds(), ["library_changed"]);
export const useConfig = () => useLive<AppConfig>((b) => b.getConfig(), ["engine_state", "profiles_changed"]);

/** Sound id → display name (built-in names are translated by the caller). */
export function useSoundIndex(sounds: SoundEntry[] | undefined) {
  return useMemo(() => new Map((sounds ?? []).map((s) => [s.id, s])), [sounds]);
}
