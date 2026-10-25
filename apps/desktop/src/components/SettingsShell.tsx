"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useTranslations } from "next-intl";
import { useEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
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

/** ⌘1–⌘5 → page id (mockup interaction notes), or null for any other key. */
export function pageForShortcut(e: Pick<KeyboardEvent, "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "key">): PageId | null {
  if (!e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return null;
  const n = Number(e.key);
  return Number.isInteger(n) && n >= 1 && n <= PAGES.length ? PAGES[n - 1].id : null;
}

/** `/library/` → "library"; anything unknown → the first page. */
export function pageFromPath(pathname: string | null): PageId {
  const seg = (pathname ?? "").split("/").find(Boolean);
  return PAGES.find((p) => p.id === seg)?.id ?? PAGES[0].id;
}

const HEADER_ACTIONS_ID = "settings-header-actions";

/** Lets a page put buttons in the shell's header (e.g. import / export on the library page). */
export function HeaderActions({ children }: { children: ReactNode }) {
  const [target, setTarget] = useState<HTMLElement | null>(null);
  useEffect(() => setTarget(document.getElementById(HEADER_ACTIONS_ID)), []);
  return target ? createPortal(children, target) : null;
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
  const router = useRouter();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const page = pageForShortcut(e);
      if (!page) return;
      e.preventDefault();
      router.push(`/${page}/`);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [router]);
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
          <div id={HEADER_ACTIONS_ID} className={styles.headerActions}>
            {headerActions}
          </div>
        </header>
        <main className={styles.content}>{children}</main>
      </div>
    </div>
  );
}
