import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { renderWithBackend } from "@/test/render";
import { GeneralPage } from "./GeneralPage";
import { OnboardingWizard } from "./OnboardingWizard";
import { ProfilesPage } from "./ProfilesPage";
import { SensorsPage } from "./SensorsPage";

afterEach(cleanup);

/** The Mac App Store edition "Creaky" plays lid sounds only (ADR-0017). */
describe("App Store edition", () => {
  it("Sensors shows the lid section but no slaps, accelerometer or Input Monitoring", async () => {
    renderWithBackend(<SensorsPage />, { edition: "appstore" });
    expect(await screen.findByRole("heading", { name: "螢幕角度" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: /拍打/ })).toBeNull();
    expect(screen.queryByText("加速度計")).toBeNull();
    expect(screen.queryByText("輸入監控")).toBeNull();
  });

  it("Profiles lists only the three lid events", async () => {
    renderWithBackend(<ProfilesPage />, { edition: "appstore" });
    await screen.findByRole("heading", { level: 2, name: "辦公室安靜版" });
    expect(screen.getAllByRole("listitem")).toHaveLength(3);
    expect(screen.queryByText("輕拍")).toBeNull();
  });

  it("General has no update controls and says nothing goes online", async () => {
    renderWithBackend(<GeneralPage />, { edition: "appstore" });
    expect(await screen.findByText(/也不會連上網路/)).toBeTruthy();
    expect(screen.getByText(/Creaky 不收集任何資料/)).toBeTruthy();
    expect(screen.queryByRole("switch", { name: "自動檢查更新" })).toBeNull();
    expect(screen.queryByRole("button", { name: "檢查更新" })).toBeNull();
  });

  it("onboarding skips the slap and calibration steps", async () => {
    const { backend } = renderWithBackend(<OnboardingWizard mode="onboarding" />, { edition: "appstore" });
    expect(screen.getByText("1 / 4")).toBeTruthy();
    expect(screen.getByText(/它都有聲音配合演出/)).toBeTruthy();
    const click = async (name: string) => {
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name }));
      });
    };
    await click("開始");
    expect(screen.getByText("2 / 4")).toBeTruthy();
    await click("下一步");
    expect(screen.getByText("3 / 4")).toBeTruthy();
    expect(screen.getByText("整台都是玻璃：開蓋時慢慢裂開，闔上清脆一響。")).toBeTruthy();
    await click("下一步");
    expect(screen.getByText("都設好了")).toBeTruthy();
    expect(screen.getByText("4 / 4")).toBeTruthy();
    expect(screen.queryByText(/拍打功能尚未啟用/)).toBeNull();
    expect(backend.data.state.calibrating).toBe(false);
  });
});
