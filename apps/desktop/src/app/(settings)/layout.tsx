import type { ReactNode } from "react";
import { SettingsFrame } from "@/components/SettingsFrame";

export default function SettingsLayout({ children }: { children: ReactNode }) {
  return <SettingsFrame>{children}</SettingsFrame>;
}
