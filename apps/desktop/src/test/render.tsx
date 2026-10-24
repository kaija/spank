import { render } from "@testing-library/react";
import { NextIntlClientProvider } from "next-intl";
import type { ReactElement } from "react";
import { BackendProvider } from "@/backend/BackendProvider";
import { createMockBackend, type MockSeed } from "@/backend/mock";
import en from "@/i18n/messages/en.json";
import zhHant from "@/i18n/messages/zh-Hant.json";

/** Render a screen with the in-memory backend and real messages. */
export function renderWithBackend(ui: ReactElement, opts: { seed?: Partial<MockSeed>; locale?: "zh-Hant" | "en" } = {}) {
  const backend = createMockBackend(opts.seed);
  const locale = opts.locale ?? "zh-Hant";
  const result = render(
    <NextIntlClientProvider locale={locale} messages={locale === "en" ? en : zhHant} timeZone="Asia/Taipei">
      <BackendProvider backend={backend}>{ui}</BackendProvider>
    </NextIntlClientProvider>,
  );
  return { ...result, backend };
}
