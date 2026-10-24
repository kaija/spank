"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useTranslations } from "next-intl";
import type { ReactNode } from "react";
import { Icon } from "@/ui/Icon";
import type { IconName } from "@/ui/iconPaths";
import styles from "./SettingsShell.module.css";

export const PAGES = [
  { id: "profiles", icon: "profiles" },
  { id: "library", icon: "library" },
  { id: "sensors", icon: "sensors" },
  { id: "mute", icon: "mute" },
  { id: "general", icon: "general" },
] as const satisfies readonly { id: string; icon: IconName }[];

export type PageId = (typeof PAGES)[number]["id"];

/** `/library/` → "library"; anything unknown → the first page. */
export function pageFromPath(pathname: string | null): PageId {
  const seg = (pathname ?? "").split("/").find(Boolean);
  return PAGES.find((p) => p.id === seg)?.id ?? PAGES[0].id;
}

export type ShellStatus = { kind: "running" } | { kind: "paused" } | { kind: "muted"; reason: string };

type Props = {
  children: ReactNode;
  status?: ShellStatus;
  /** Right side of the page header (e.g. import / export buttons on the library page). */
  headerActions?: ReactNode;
};

/**
 * Settings window frame from mockup/Settings Window.dc.html: 188px sidebar with page nav and
 * status, 52px page header, scrolling content. The window uses an overlay title bar, so the
 * native traffic lights sit in the sidebar's top padding, which doubles as a drag region.
 */
export function SettingsShell({ children, status = { kind: "running" }, headerActions }: Props) {
  const t = useTranslations();
  const current = pageFromPath(usePathname());
  const statusText =
    status.kind === "muted" ? t("status.muted", { reason: status.reason }) : t(`status.${status.kind}`);

  return (
    <div className={styles.window} role="application" aria-label={t("app.settingsWindow", { appName: t("app.name") })}>
      <nav className={styles.sidebar} aria-label={t("nav.label")}>
        <div className={styles.titlebar} data-tauri-drag-region />
        <ul className={styles.navList}>
          {PAGES.map((p) => (
            <li key={p.id}>
              <Link
                href={`/${p.id}/`}
                className={styles.navItem}
                aria-current={p.id === current ? "page" : undefined}
              >
                <Icon name={p.icon} />
                <span>{t(`nav.${p.id}`)}</span>
              </Link>
            </li>
          ))}
        </ul>
        <div className={styles.status} role="status">
          <span className={styles.statusDot} data-kind={status.kind} />
          <span>{statusText}</span>
        </div>
      </nav>
      <div className={styles.main}>
        <header className={styles.header} data-tauri-drag-region>
          <h1 className={styles.title}>{t(`nav.${current}`)}</h1>
          {headerActions && <div className={styles.headerActions}>{headerActions}</div>}
        </header>
        <main className={styles.content}>{children}</main>
      </div>
    </div>
  );
}
