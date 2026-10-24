import type { Language } from "@/bindings/Language";
import en from "./messages/en.json";
import zhHant from "./messages/zh-Hant.json";

export type Locale = "zh-Hant" | "en";

export const DEFAULT_LOCALE: Locale = "zh-Hant";

export const MESSAGES: Record<Locale, typeof zhHant> = { "zh-Hant": zhHant, en };

/**
 * Pick the UI locale from the user's setting, falling back to the system language list
 * (the WebView's `navigator.languages` follows macOS). Any Chinese variant maps to zh-Hant,
 * everything else to English.
 */
export function resolveLocale(setting: Language, systemLanguages: readonly string[]): Locale {
  if (setting === "zh-hant") return "zh-Hant";
  if (setting === "en") return "en";
  for (const tag of systemLanguages) {
    const lower = tag.toLowerCase();
    if (lower.startsWith("zh")) return "zh-Hant";
    if (lower.startsWith("en")) return "en";
  }
  return "en";
}
