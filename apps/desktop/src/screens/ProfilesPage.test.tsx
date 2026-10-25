import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { renderWithBackend } from "@/test/render";
import { ProfilesPage } from "./ProfilesPage";

afterEach(cleanup);

const option = (name: string) => screen.getByRole("option", { name: new RegExp(name) });
const row = (label: string) => screen.getByText(label).closest('[role="listitem"]') as HTMLElement;

describe("ProfilesPage", () => {
  it("lists built-in and custom profiles and opens the active one", async () => {
    renderWithBackend(<ProfilesPage />);
    expect(await screen.findByRole("heading", { level: 2, name: "辦公室安靜版" })).toBeTruthy();
    expect(option("全玻璃").textContent).toContain("慢慢裂開、清脆碎掉");
    expect(option("辦公室安靜版").textContent).toContain("使用中");
    expect(screen.getAllByRole("listitem")).toHaveLength(7);
    // Chips show localized built-in sound names.
    expect(within(row("打開螢幕")).getByText("石塊鬆動")).toBeTruthy();
    expect(within(row("重拍")).getByText("好痛 02")).toBeTruthy();
  });

  it("built-in profiles are read-only and can be duplicated", async () => {
    const { backend } = renderWithBackend(<ProfilesPage />);
    fireEvent.click(await screen.findByRole("option", { name: /全玻璃/ }));
    expect(screen.getByRole("note").textContent).toContain("內建 Profile 無法編輯");
    expect(screen.queryByRole("button", { name: /加入音效/ })).toBeNull();
    expect((screen.getByRole("button", { name: "刪除 Profile" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("slider", { name: "Profile 音量" }) as HTMLInputElement).disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "複製後編輯" }));
    const input = (await screen.findByRole("textbox", { name: "重新命名 Profile" })) as HTMLInputElement;
    expect(input.value).toBe("全玻璃 拷貝");
    fireEvent.change(input, { target: { value: "我的玻璃" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(backend.data.profiles.some((p) => p.name === "我的玻璃" && !p.builtin)).toBe(true));
  });

  it("edits a custom profile: add and remove sounds, toggle, volume", async () => {
    const { backend } = renderWithBackend(<ProfilesPage />);
    await screen.findByRole("heading", { name: "辦公室安靜版" });
    const office = () => backend.data.profiles.find((p) => p.id === "user-office")!;

    fireEvent.click(screen.getByRole("button", { name: "為「開啟中」加入音效" }));
    fireEvent.click(within(screen.getByRole("menu")).getByRole("menuitem", { name: /石門開啟 01/ }));
    await waitFor(() => expect(office().mapping.lid_overbent?.sounds).toEqual(["builtin.stone-open-01"]));
    expect(within(row("開啟中")).getByText("石門開啟 01")).toBeTruthy();

    fireEvent.click(within(row("闔上螢幕")).getByRole("button", { name: "移除 玻璃裂響" }));
    await waitFor(() => expect(office().mapping.lid_closing?.sounds).toEqual([]));

    fireEvent.click(screen.getByRole("switch", { name: "輕拍：已啟用" }));
    await waitFor(() => expect(office().mapping.slap_light?.enabled).toBe(false));

    const slider = screen.getByRole("slider", { name: "Profile 音量" });
    fireEvent.change(slider, { target: { value: "35" } });
    fireEvent.pointerUp(slider, { target: { value: "35" } });
    await waitFor(() => expect(office().volume).toBeCloseTo(0.35));
  });

  it("previews a chip and deletes a profile after confirming", async () => {
    const { backend } = renderWithBackend(<ProfilesPage />);
    await screen.findByRole("heading", { name: "辦公室安靜版" });
    fireEvent.click(within(row("打開螢幕")).getByRole("button", { name: "試聽 石塊鬆動" }));
    expect(backend.data.log).toContain("preview:builtin.stone-wake-01");
    expect(within(row("打開螢幕")).getByRole("button", { name: "停止 石塊鬆動" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "刪除 Profile" }));
    const sheet = screen.getByRole("alertdialog");
    expect(sheet.textContent).toContain("這是目前使用中的 Profile");
    fireEvent.click(within(sheet).getByRole("button", { name: "刪除" }));
    await waitFor(() => expect(backend.data.profiles.some((p) => p.id === "user-office")).toBe(false));
    expect(backend.data.config.active_profile).toBe("builtin.destruction");
  });

  it("creates a new profile and can make it active", async () => {
    const { backend } = renderWithBackend(<ProfilesPage />, { locale: "en" });
    await screen.findByRole("heading", { name: "辦公室安靜版" });
    fireEvent.click(screen.getByRole("button", { name: "New profile" }));
    const input = (await screen.findByRole("textbox", { name: "Rename profile" })) as HTMLInputElement;
    expect(input.value).toBe("Untitled profile");
    fireEvent.keyDown(input, { key: "Escape" });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Use this profile" }));
    });
    const created = backend.data.profiles.find((p) => p.name === "Untitled profile")!;
    await waitFor(() => expect(backend.data.config.active_profile).toBe(created.id));
    expect(screen.getAllByText("No sound yet — this event will happen silently")).toHaveLength(7);
  });
});
