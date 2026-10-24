import "@fontsource-variable/inter";
import type { Metadata } from "next";
import type { ReactNode } from "react";
import { I18nProvider } from "@/i18n/I18nProvider";
import "./globals.css";

export const metadata: Metadata = { title: "Spank" };

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="zh-Hant">
      <body>
        <I18nProvider>{children}</I18nProvider>
      </body>
    </html>
  );
}
