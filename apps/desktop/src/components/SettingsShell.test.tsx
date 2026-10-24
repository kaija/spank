import { cleanup, render, screen } from "@testing-library/react";
import { NextIntlClientProvider } from "next-intl";
import { afterEach, describe, expect, it, vi } from "vitest";
import en from "@/i18n/messages/en.json";
import zhHant from "@/i18n/messages/zh-Hant.json";
import { PAGES, SettingsShell, pageFromPath } from "./SettingsShell";

let pathname = "/profiles/";
vi.mock("next/navigation", () => ({ usePathname: () => pathname }));

function renderShell(messages: typeof zhHant, locale: string, props: Partial<Parameters<typeof SettingsShell>[0]> = {}) {
  return render(
    <NextIntlClientProvider locale={locale} messages={messages}>
      <SettingsShell {...props}>
        <p>content</p>
      </SettingsShell>
    </NextIntlClientProvider>,
  );
}

afterEach(cleanup);

describe("pageFromPath", () => {
  it("maps routes to pages", () => {
    expect(pageFromPath("/library/")).toBe("library");
    expect(pageFromPath("/general")).toBe("general");
    expect(pageFromPath("/")).toBe("profiles");
    expect(pageFromPath("/nope/")).toBe("profiles");
    expect(pageFromPath(null)).toBe("profiles");
  });
});

describe("SettingsShell", () => {
  it("renders the five pages in mockup order and marks the current one", () => {
    pathname = "/sensors/";
    renderShell(zhHant, "zh-Hant");
    const links = screen.getAllByRole("link");
    expect(links.map((l) => l.textContent)).toEqual(["Profiles", "音效庫", "感測器", "自動靜音", "一般"]);
    // next/link only appends the trailing slash when next.config (trailingSlash) is loaded.
    expect(links.map((l) => l.getAttribute("href")?.replace(/\/$/, ""))).toEqual(PAGES.map((p) => `/${p.id}`));
    expect(screen.getByRole("link", { name: "感測器" }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("感測器");
    expect(screen.getByText("content")).toBeTruthy();
  });

  it("shows status text", () => {
    pathname = "/profiles/";
    renderShell(zhHant, "zh-Hant", { status: { kind: "muted", reason: "開會中" } });
    expect(screen.getByRole("status").textContent).toBe("已靜音 — 開會中");
  });

  it("renders in English", () => {
    pathname = "/library/";
    renderShell(en, "en", { headerActions: <button type="button">Import…</button> });
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Sound library");
    expect(screen.getByRole("status").textContent).toBe("Running");
    expect(screen.getByRole("button", { name: "Import…" })).toBeTruthy();
    expect(screen.getByRole("application").getAttribute("aria-label")).toBe("Spank Settings");
  });
});
