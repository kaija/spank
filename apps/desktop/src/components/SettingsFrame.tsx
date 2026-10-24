"use client";

import { useTranslations } from "next-intl";
import type { ReactNode } from "react";
import { useRuntimeState } from "@/backend/BackendProvider";
import { SettingsShell, type ShellStatus } from "./SettingsShell";

function LiveShell({ children }: { children: ReactNode }) {
  const t = useTranslations();
  const state = useRuntimeState();
  let status: ShellStatus = { kind: "running" };
  if (state?.paused) status = { kind: "paused" };
  else if (state?.mute.reasons.length) status = { kind: "muted", reason: t(`reasons.${state.mute.reasons[0]}`) };
  return <SettingsShell status={status}>{children}</SettingsShell>;
}

/** Settings window frame wired to the app (the `BackendProvider` lives in the root layout). */
export function SettingsFrame({ children }: { children: ReactNode }) {
  return <LiveShell>{children}</LiveShell>;
}
