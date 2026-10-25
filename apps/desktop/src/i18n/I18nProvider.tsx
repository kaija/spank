"use client";

import { NextIntlClientProvider } from "next-intl";
import { useEffect, useState, type ReactNode } from "react";
import { useConfig } from "@/backend/BackendProvider";
import { useEdition, withProductName } from "@/lib/edition";
import type { Language } from "@/bindings/Language";
import { DEFAULT_LOCALE, MESSAGES, resolveLocale, type Locale } from "./locale";

/**
 * Client-side i18n (static export: no i18n routing, no server).
 * Pages prerender in zh-Hant; on mount we switch to the resolved locale.
 */
export function I18nProvider({ children, setting = "system" }: { children: ReactNode; setting?: Language }) {
  const [locale, setLocale] = useState<Locale>(DEFAULT_LOCALE);
  const edition = useEdition();

  useEffect(() => {
    const next = resolveLocale(setting, navigator.languages ?? [navigator.language]);
    setLocale(next);
    document.documentElement.lang = next;
  }, [setting]);

  return (
    <NextIntlClientProvider locale={locale} messages={withProductName(MESSAGES[locale], edition)} timeZone="Asia/Taipei">
      {children}
    </NextIntlClientProvider>
  );
}

/** Follows the app's language setting (Settings › General); needs a `BackendProvider` above. */
export function AppI18nProvider({ children }: { children: ReactNode }) {
  const [config] = useConfig();
  return <I18nProvider setting={config?.language ?? "system"}>{children}</I18nProvider>;
}
