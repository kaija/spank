import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { renderWithBackend } from "@/test/render";
import { GeneralPage } from "./GeneralPage";

afterEach(cleanup);

describe("GeneralPage", () => {
  it("saves switches, volume and language", async () => {
    const { backend } = renderWithBackend(<GeneralPage />);
    const login = await screen.findByRole("switch", { name: "登入時啟動" });
    fireEvent.click(login);
    await waitFor(() => expect(backend.data.config.launch_at_login).toBe(false));
    fireEvent.click(screen.getByRole("switch", { name: "自動檢查更新" }));
    await waitFor(() => expect(backend.data.config.auto_check_updates).toBe(false));

    const volume = screen.getByRole("slider", { name: "App 總音量" });
    fireEvent.change(volume, { target: { value: "35" } });
    fireEvent.pointerUp(volume, { target: { value: "35" } });
    await waitFor(() => expect(backend.data.config.app_volume).toBeCloseTo(0.35));

    fireEvent.change(screen.getByLabelText("語言"), { target: { value: "en" } });
    await waitFor(() => expect(backend.data.config.language).toBe("en"));
    expect(await screen.findByText("0.1.0 · macOS 14+ · Apple Silicon")).toBeTruthy();
  });

  it("copies diagnostics and opens the log folder", async () => {
    const { backend } = renderWithBackend(<GeneralPage />);
    const copy = await screen.findByRole("button", { name: "複製診斷資訊" });
    await act(async () => {
      fireEvent.click(copy);
    });
    expect(await screen.findByText("已複製")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "打開 log 資料夾" }));
    expect(backend.data.log).toEqual(["diagnostics", "log-folder"]);
  });

  it("shows an error and keeps the old value when saving fails", async () => {
    const { backend } = renderWithBackend(<GeneralPage />);
    backend.setLaunchAtLogin = async () => {
      throw new Error("not allowed");
    };
    fireEvent.click(await screen.findByRole("switch", { name: "登入時啟動" }));
    expect((await screen.findByRole("alert")).textContent).toContain("not allowed");
    expect((screen.getByRole("switch", { name: "登入時啟動" }) as HTMLInputElement).checked).toBe(true);
  });
});
