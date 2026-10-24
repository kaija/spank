"use client";

import { OnboardingWizard } from "@/screens/OnboardingWizard";

/** Calibration-only window (Settings › Sensors › Recalibrate). */
export default function Page() {
  return <OnboardingWizard mode="calibration" />;
}
