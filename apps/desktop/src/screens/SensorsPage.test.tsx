import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RuntimeState } from "@/bindings/RuntimeState";
import { createMockBackend } from "@/backend/mock";
import { renderWithBackend } from "@/test/render";
import { SensorsPage } from "./SensorsPage";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const base = createMockBackend().data;
const state = (patch: Partial<RuntimeState>): RuntimeState => ({ ...base.state, ...patch });
const calibration = { levels: { light_g: 0.2, medium_g: 0.4, hard_g: 0.8 }, noise_floor_g: 0.02, calibrated_unix_ms: 0 };

describe("SensorsPage", () => {
  it("shows the live angle and edits thresholds", async () => {
    const { backend, unmount } = renderWithBackend(<SensorsPage />);
    expect(await screen.findByText("104°")).toBeTruthy();
    expect(screen.getByText("正常使用中 · 即時")).toBeTruthy();
    expect(backend.data.log).toContain("monitor:true");

    const close = screen.getAllByRole("slider", { name: "闔上門檻（度）" });
    // Arrow keys on the gauge handle: +5 with Shift.
    fireEvent.keyDown(close[0], { key: "ArrowUp", shiftKey: true });
    await waitFor(() => expect(backend.data.config.lid.close_threshold_deg).toBe(25), { timeout: 1500 });
    // The range input next to it.
    fireEvent.change(close[1], { target: { value: "30" } });
    await waitFor(() => expect(backend.data.config.lid.close_threshold_deg).toBe(30), { timeout: 1500 });

    // Invalid combinations are shown and not saved.
    fireEvent.change(screen.getAllByRole("slider", { name: "開啟上限門檻（度）" })[1], { target: { value: "20" } });
    expect((await screen.findByRole("alert")).textContent).toContain("無法儲存");
    await new Promise((r) => setTimeout(r, 500));
    expect(backend.data.config.lid.overbend_threshold_deg).toBe(120);

    unmount();
    expect(backend.data.log).toContain("monitor:false");
  });

  it("explains a Mac without a lid sensor", async () => {
    renderWithBackend(<SensorsPage />, {
      seed: { state: state({ caps: { lid: { state: "unsupported", reason: "x" }, accel: { state: "available" } } }) },
    });
    expect(await screen.findByText(/沒有螢幕角度感測器/)).toBeTruthy();
    expect(screen.getAllByText("不支援")).toHaveLength(1);
  });

  it("opens calibration instead of enabling slaps when uncalibrated", async () => {
    const { backend } = renderWithBackend(<SensorsPage />);
    expect(await screen.findByText(/尚未校準/)).toBeTruthy();
    fireEvent.click(screen.getByRole("switch", { name: "啟用拍打偵測" }));
    expect(backend.data.log).toContain("open-calibration");
    expect(backend.data.config.slap.enabled).toBe(false);
  });

  it("toggles slaps, shows the last slap and scales sensitivity", async () => {
    const { backend } = renderWithBackend(<SensorsPage />, { seed: { calibration } });
    const sw = await screen.findByRole("switch", { name: "啟用拍打偵測" });
    expect(screen.queryByText(/尚未校準/)).toBeNull();
    expect(screen.getByRole("button", { name: "重新校準…" })).toBeTruthy();
    fireEvent.click(sw);
    await waitFor(() => expect(backend.data.config.slap.enabled).toBe(true));

    act(() => {
      backend.emit("trigger_fired", {
        event: { kind: "slap_medium", at_ms: 1, meta: { type: "slap", peak_g: 0.5 } },
        result: { type: "played", sound: "builtin.aiyo" },
      });
    });
    expect(screen.getByText("剛剛偵測到：中拍")).toBeTruthy();

    const sens = screen.getByRole("slider", { name: "靈敏度" });
    fireEvent.change(sens, { target: { value: "100" } });
    fireEvent.pointerUp(sens, { target: { value: "100" } });
    await waitFor(() => expect(backend.data.config.slap.levels).toEqual({ light_g: 0.1, medium_g: 0.2, hard_g: 0.4 }));
    expect(backend.data.config.slap.trigger_g).toBe(0.1);
  });

  it("asks for Input Monitoring for precise typing, and explains a denial", async () => {
    const { backend } = renderWithBackend(<SensorsPage />, { seed: { inputAnswer: "denied" } });
    const precise = await screen.findByRole("switch", { name: "精準打字模式" });
    await act(async () => {
      fireEvent.click(precise);
    });
    await waitFor(() => expect(backend.data.config.slap.precise_typing).toBe(true));
    expect(backend.data.state.input_access).toBe("denied");
    expect(await screen.findByText(/尚未取得輸入監控權限/)).toBeTruthy();
    expect(screen.getByText("未授權")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "打開系統設定" }));
    expect(backend.data.log).toContain("input-settings");
  });
});
