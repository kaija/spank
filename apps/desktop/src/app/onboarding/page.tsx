"use client";

import { OnboardingWizard } from "@/screens/OnboardingWizard";

/** First-run window (opened by the app when `onboarding_done` is false). */
export default function Page() {
  return <OnboardingWizard mode="onboarding" />;
}
