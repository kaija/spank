import { render } from "@testing-library/react";
import { NextIntlClientProvider } from "next-intl";
import type { ReactElement } from "react";
import { BackendProvider } from "@/backend/BackendProvider";
import { createMockBackend, type MockSeed } from "@/backend/mock";
import { EditionContext, withProductName, type Edition } from "@/lib/edition";
import en from "@/i18n/messages/en.json";
import zhHant from "@/i18n/messages/zh-Hant.json";

/** Render a screen with the in-memory backend and real messages, in the DMG edition unless `edition` says otherwise. */
export function renderWithBackend(
  ui: ReactElement,
  opts: { seed?: Partial<MockSeed>; locale?: "zh-Hant" | "en"; edition?: Edition } = {},
) {
  const edition = opts.edition ?? "direct";
  const backend = createMockBackend(opts.seed, { edition });
  const locale = opts.locale ?? "zh-Hant";
  const result = render(
    <EditionContext.Provider value={edition}>
      <NextIntlClientProvider locale={locale} messages={withProductName(locale === "en" ? en : zhHant, edition)} timeZone="Asia/Taipei">
        <BackendProvider backend={backend}>{ui}</BackendProvider>
      </NextIntlClientProvider>
    </EditionContext.Provider>,
  );
  return { ...result, backend };
}
