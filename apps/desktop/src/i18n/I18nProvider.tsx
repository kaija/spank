"use client";

import { NextIntlClientProvider } from "next-intl";
import { useEffect, useState, type ReactNode } from "react";
import type { Language } from "@/bindings/Language";
import { DEFAULT_LOCALE, MESSAGES, resolveLocale, type Locale } from "./locale";

/**
 * Client-side i18n (static export: no i18n routing, no server).
 * Pages prerender in zh-Hant; on mount we switch to the resolved locale.
 * `setting` will come from AppConfig.language once settings are wired up (T38).
 */
export function I18nProvider({ children, setting = "system" }: { children: ReactNode; setting?: Language }) {
  const [locale, setLocale] = useState<Locale>(DEFAULT_LOCALE);

  useEffect(() => {
    const next = resolveLocale(setting, navigator.languages ?? [navigator.language]);
    setLocale(next);
    document.documentElement.lang = next;
  }, [setting]);

  return (
    <NextIntlClientProvider locale={locale} messages={MESSAGES[locale]} timeZone="Asia/Taipei">
      {children}
    </NextIntlClientProvider>
  );
}
