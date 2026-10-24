"use client";

import { useTranslations } from "next-intl";

/** Stand-in for pages that later tasks implement. */
export function PagePlaceholder({ task }: { task: string }) {
  const t = useTranslations("placeholder");
  return (
    <div style={{ padding: "48px 24px", textAlign: "center", color: "var(--text-muted)" }}>
      <p style={{ fontWeight: 600, color: "var(--text-body)", marginBottom: 6 }}>{t("title")}</p>
      <p>{t("body", { task })}</p>
    </div>
  );
}
