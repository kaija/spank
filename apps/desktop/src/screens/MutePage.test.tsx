import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RuntimeState } from "@/bindings/RuntimeState";
import { createMockBackend } from "@/backend/mock";
import { renderWithBackend } from "@/test/render";
import { MutePage, timelinePieces } from "./MutePage";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const baseState = createMockBackend().data.state;
const state = (patch: Partial<RuntimeState>): RuntimeState => ({ ...baseState, ...patch });

describe("MutePage", () => {
  it("shows why it is muted and marks the firing rule", async () => {
    renderWithBackend(<MutePage />, {
      seed: { state: state({ mute: { reasons: ["meeting", "quiet_hours"], focus_unavailable: false } }) },
    });
    const banner = await screen.findByRole("status");
    expect(banner.textContent).toContain("目前已靜音 — 開會中、安靜時段");
    expect(screen.getAllByText("成立中")).toHaveLength(2);
  });

  it("toggles rules and saves them", async () => {
    const { backend } = renderWithBackend(<MutePage />);
    await screen.findByText("目前會出聲");
    fireEvent.click(screen.getByRole("switch", { name: "跟隨系統靜音" }));
    await waitFor(() => expect(backend.data.config.mute.follow_system).toBe(false));
    fireEvent.click(screen.getByRole("switch", { name: "開會中" }));
    await waitFor(() => expect(backend.data.config.mute.meeting).toBe(false));
  });

  it("asks for Focus permission, and explains a denial", async () => {
    const { backend } = renderWithBackend(<MutePage />);
    const allow = await screen.findByRole("button", { name: "允許存取…" });
    await act(async () => {
      fireEvent.click(allow);
    });
    await waitFor(() => expect(screen.queryByRole("button", { name: "允許存取…" })).toBeNull());
    expect(backend.data.state.focus_authorization).toBe("authorized");
    cleanup();

    const denied = renderWithBackend(<MutePage />, { seed: { state: state({ focus_authorization: "denied" }) } });
    fireEvent.click(await screen.findByRole("button", { name: "打開系統設定" }));
    expect(denied.backend.data.log).toContain("focus-settings");
    expect((screen.getByRole("switch", { name: "專注模式" }) as HTMLInputElement).disabled).toBe(true);
    cleanup();

    renderWithBackend(<MutePage />, {
      seed: { state: state({ focus_authorization: "authorized", mute: { reasons: [], focus_unavailable: true } }) },
    });
    expect(await screen.findByText("這個版本讀不到專注模式狀態（需要正式簽章的版本）。其他規則照常運作。")).toBeTruthy();
  });

  it("edits quiet hours", async () => {
    const { backend } = renderWithBackend(<MutePage />);
    await screen.findByText("目前會出聲");
    expect(screen.getByText("跨午夜 · 隔天")).toBeTruthy();
    expect(screen.getByText("8 小時 30 分")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "新增時段" }));
    await waitFor(() => expect(backend.data.config.mute.quiet_hours).toHaveLength(3));
    const names = screen.getAllByRole("textbox", { name: "時段名稱" });
    fireEvent.change(names[2], { target: { value: "開會" } });
    fireEvent.change(screen.getAllByLabelText("結束時間")[2], { target: { value: "11:15" } });
    await waitFor(() =>
      expect(backend.data.config.mute.quiet_hours[2]).toEqual({ start: "09:00", end: "11:15", label: "開會" }),
    );

    fireEvent.click(screen.getByRole("button", { name: "刪除時段「午休」" }));
    await waitFor(() => expect(backend.data.config.mute.quiet_hours.map((r) => r.label)).toEqual(["睡覺", "開會"]));
    expect(within(screen.getByRole("region", { name: "安靜時段" })).getAllByRole("textbox")).toHaveLength(2);

    // An empty range (start == end) stays local and is not saved.
    fireEvent.change(screen.getAllByLabelText("結束時間")[1], { target: { value: "09:00" } });
    await new Promise((r) => setTimeout(r, 500));
    expect(backend.data.config.mute.quiet_hours[1].end).toBe("11:15");
  });
});

describe("timelinePieces", () => {
  it("splits ranges across midnight", () => {
    const p = timelinePieces([
      { start: "12:00", end: "18:00", label: "a" },
      { start: "18:00", end: "06:00", label: "b" },
    ]);
    expect(p.map((x) => [x.left, x.width, x.label, x.cut])).toEqual([
      ["50.00%", "25.00%", "a", "none"],
      ["75.00%", "25.00%", "b →", "right"],
      ["0%", "25.00%", "→ b", "left"],
    ]);
  });
});
