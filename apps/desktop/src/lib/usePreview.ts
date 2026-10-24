"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useBackend } from "@/backend/BackendProvider";
import type { SoundEntry } from "@/bindings/SoundEntry";

/**
 * Play / stop one sound at a time. `playing` is the caller's key (a sound id, or "event:sound")
 * and clears itself when the sound should have finished.
 */
export function usePreview() {
  const backend = useBackend();
  const [playing, setPlaying] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  const toggle = useCallback(
    (key: string, sound: SoundEntry | undefined) => {
      clearTimeout(timer.current);
      if (playing === key || !sound) {
        setPlaying(null);
        void backend.stopPreview();
        return;
      }
      setPlaying(key);
      void backend.previewSound(sound.id);
      timer.current = setTimeout(() => setPlaying(null), Math.max(400, sound.duration_ms + 150));
    },
    [backend, playing],
  );
  return { playing, toggle };
}
