import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { CalibrationProgress } from "@/bindings/CalibrationProgress";
import { createMockBackend } from "@/backend/mock";
import { renderWithBackend } from "@/test/render";
import { calStatusFor, calTarget, OnboardingWizard } from "./OnboardingWizard";

afterEach(cleanup);

const click = async (name: string) => {
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name }));
  });
};

const progress = (patch: Partial<CalibrationProgress>): CalibrationProgress => ({
  step: "light",
  status: { status: "waiting" },
  light_g: null,
  medium_g: null,
  hard_g: null,
  typing_g: null,
  ...patch,
});

describe("OnboardingWizard", () => {
  it("walks from welcome through calibration to done", async () => {
    const { backend } = renderWithBackend(<OnboardingWizard mode="onboarding" />, {
      seed: { config: { ...createMockBackend().data.config, active_profile: "builtin.destruction" } },
    });
    expect(screen.getByText("1 / 6")).toBeTruthy();
    await click("開始");

    // Lid demo: live angle and feedback from a fired lid event.
    expect(await screen.findByText("104°")).toBeTruthy();
    await click("播放示範音效");
    expect(backend.data.log).toContain("event:lid_opened");
    act(() =>
      backend.emit("trigger_fired", {
        event: { kind: "lid_opened", at_ms: 1, meta: { type: "none" } },
        result: { type: "played", sound: "builtin.oh" },
      }),
    );
    expect(screen.getByText("偵測到了！剛剛播放「喔！」")).toBeTruthy();
    await click("下一步");

    // Profile cards: picking one switches the active profile.
    expect(screen.getByRole("radio", { name: /毀滅/ }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(screen.getByRole("radio", { name: /哀哀叫/ }));
    await waitFor(() => expect(backend.data.config.active_profile).toBe("builtin.whine"));
    await click("試聽 經典");
    expect(backend.data.log).toContain("preview:builtin.thud-low");
    await click("下一步");

    await click("啟用並校準");
    expect(backend.data.state.calibrating).toBe(true);
    const next = () => screen.getByRole("button", { name: /下一個|完成校準/ }) as HTMLButtonElement;
    expect(next().disabled).toBe(true);

    // Engine reports each step.
    act(() => backend.emit("calibration", progress({ status: { status: "success", peak_g: 0.2 }, light_g: 0.2 })));
    expect(screen.getByText("收到！輕拍力道約 0.20 g")).toBeTruthy();
    await click("下一個");
    expect(backend.data.log).toContain("cal-step:medium");
    act(() =>
      backend.emit("calibration", progress({ step: "medium", status: { status: "too_weak", peak_g: 0.21 }, light_g: 0.2 })),
    );
    expect(screen.getByText("有點太輕，跟輕拍差不多。再大力一點。")).toBeTruthy();
    expect(next().disabled).toBe(true);
    await click("重來");
    expect(backend.data.log).toContain("cal-step:medium:reset");
    act(() =>
      backend.emit(
        "calibration",
        progress({ step: "medium", status: { status: "success", peak_g: 0.4 }, light_g: 0.2, medium_g: 0.4 }),
      ),
    );
    await click("下一個");
    act(() =>
      backend.emit(
        "calibration",
        progress({ step: "hard", status: { status: "success", peak_g: 0.9 }, light_g: 0.2, medium_g: 0.4, hard_g: 0.9 }),
      ),
    );
    await click("下一個");

    // Typing: recording starts on the first keystroke.
    expect(screen.getByText("等待中")).toBeTruthy();
    await act(async () => {
      fireEvent.keyDown(screen.getByRole("textbox", { name: "在這裡隨意打字" }), { key: "a" });
    });
    expect(backend.data.log).toContain("cal-step:typing:reset");
    expect(screen.getByText("記錄中…")).toBeTruthy();
    act(() =>
      backend.emit(
        "calibration",
        progress({ step: "typing", status: { status: "success", peak_g: 0.03 }, light_g: 0.2, medium_g: 0.4, hard_g: 0.9, typing_g: 0.03 }),
      ),
    );
    backend.data.calProgress = progress({ step: "typing", light_g: 0.2, medium_g: 0.4, hard_g: 0.9, typing_g: 0.03 });
    await click("完成校準");
    expect(backend.data.config.slap.enabled).toBe(true);
    expect(backend.data.calibration?.levels.light_g).toBeCloseTo(0.12);

    expect(await screen.findByText("都設好了")).toBeTruthy();
    expect(screen.queryByText(/拍打功能尚未啟用/)).toBeNull();
    await click("完成");
    expect(backend.data.config.onboarding_done).toBe(true);
  });

  it("skipping slaps goes straight to done with a note", async () => {
    const { backend } = renderWithBackend(<OnboardingWizard mode="onboarding" />);
    await click("開始");
    await click("下一步");
    await click("下一步");
    await click("稍後再說");
    expect(screen.getByText("都設好了")).toBeTruthy();
    expect(screen.getByText("拍打功能尚未啟用，可在「設定 › 感測器」打開。")).toBeTruthy();
    expect(backend.data.state.calibrating).toBe(false);
  });

  it("going back from the first calibration step cancels it", async () => {
    const { backend } = renderWithBackend(<OnboardingWizard mode="onboarding" />);
    for (const b of ["開始", "下一步", "下一步", "啟用並校準"]) await click(b);
    expect(backend.data.state.calibrating).toBe(true);
    await click("上一步");
    expect(screen.getByText("要讓它怕痛嗎？")).toBeTruthy();
    expect(backend.data.state.calibrating).toBe(false);
  });

  it("calibration-only mode starts right away and closes when finished", async () => {
    const { backend } = renderWithBackend(<OnboardingWizard mode="calibration" />);
    await waitFor(() => expect(backend.data.state.calibrating).toBe(true));
    expect(screen.queryByText(/\/ 6/)).toBeNull();
    expect(screen.queryByRole("button", { name: "上一步" })).toBeNull();
    act(() => backend.emit("calibration", progress({ status: { status: "timeout" } })));
    expect(screen.getByText(/10 秒內沒有偵測到拍打/)).toBeTruthy();

    const done = { light_g: 0.2, medium_g: 0.4, hard_g: 0.9, typing_g: 0.02 };
    for (const step of ["light", "medium", "hard"] as const) {
      act(() => backend.emit("calibration", progress({ step, status: { status: "success", peak_g: done[`${step}_g`] }, ...done })));
      await click("下一個");
    }
    await act(async () => {
      fireEvent.keyDown(screen.getByRole("textbox", { name: "在這裡隨意打字" }), { key: "a" });
    });
    backend.data.calProgress = progress({ step: "typing", ...done });
    act(() => backend.emit("calibration", progress({ step: "typing", status: { status: "success", peak_g: 0.02 }, ...done })));
    await click("完成校準");
    expect(backend.data.log).toContain("onboarding-done");
  });

  it("explains a Mac whose accelerometer can't be used", async () => {
    const seed = createMockBackend().data.state;
    renderWithBackend(<OnboardingWizard mode="calibration" />, {
      seed: { state: { ...seed, caps: { ...seed.caps, accel: { state: "unsupported", reason: "x" } } } },
    });
    expect(await screen.findByText("這台 Mac 的加速度計無法使用，所以沒辦法校準拍打。")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "下一個" })).toBeNull();
  });
});

describe("calibration helpers", () => {
  it("shows the engine status only for the current step", () => {
    const p = progress({ step: "medium", status: { status: "success", peak_g: 0.4 } });
    expect(calStatusFor("medium", p, false)).toEqual({ status: "success", peak_g: 0.4 });
    expect(calStatusFor("hard", p, false)).toEqual({ status: "waiting" });
    expect(calStatusFor("typing", progress({ step: "typing", status: { status: "recording", progress: 0.5 } }), false)).toEqual({
      status: "waiting",
    });
    expect(calStatusFor("light", null, false)).toEqual({ status: "waiting" });
  });

  it("targets a level above the previous slap", () => {
    expect(calTarget("light", null)).toBe(0.06);
    expect(calTarget("medium", progress({ light_g: 0.2 }))).toBeCloseTo(0.24);
    expect(calTarget("hard", progress({ light_g: 0.2, medium_g: 0.5 }))).toBeCloseTo(0.6);
    expect(calTarget("typing", null)).toBeNull();
  });
});
