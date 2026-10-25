import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { renderWithBackend } from "@/test/render";
import { LibraryPage, wavePath } from "./LibraryPage";

afterEach(cleanup);

const rowOf = (name: string) => screen.getByText(name).closest('[role="row"]') as HTMLElement;

describe("LibraryPage", () => {
  it("lists sounds with localized names, source and usage", async () => {
    renderWithBackend(<LibraryPage />);
    const glass = rowOf(await screen.findByText("玻璃碎裂 01").then((e) => e.textContent!));
    expect(glass.textContent).toContain("0.8s");
    expect(glass.textContent).toContain("內建");
    expect(glass.textContent).toContain("毀滅");
    expect(rowOf("好痛 02").textContent).toContain("自行上傳");
    expect(rowOf("好痛 02").textContent).toContain("辦公室安靜版");
    expect(rowOf("玻璃大爆炸").textContent).toContain("毀滅");
  });

  it("filters by source and searches", async () => {
    renderWithBackend(<LibraryPage />);
    await screen.findByText("好痛 02");
    fireEvent.click(screen.getByRole("tab", { name: "自行上傳" }));
    expect(screen.getAllByRole("row")).toHaveLength(2); // header + 1
    fireEvent.click(screen.getByRole("tab", { name: "全部" }));
    fireEvent.change(screen.getByRole("searchbox", { name: "搜尋音效" }), { target: { value: "玻璃" } });
    expect(screen.getAllByRole("row")).toHaveLength(8); // header + 玻璃碎裂 01/02, 玻璃裂響, 玻璃大爆炸, 玻璃緊繃碎裂 01–03
    fireEvent.change(screen.getByRole("searchbox", { name: "搜尋音效" }), { target: { value: "鴨子" } });
    expect(screen.getByText("找不到「鴨子」。也許它還沒被錄下來。")).toBeTruthy();
  });

  it("imports picked files and reports each result", async () => {
    const { backend } = renderWithBackend(<LibraryPage />, {
      seed: { pickFiles: ["/x/慘叫.wav", "/x/long speech.m4a", "/x/huge.mp3", "/x/鬧鐘.aiff"] },
    });
    await screen.findByText("好痛 02");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "選擇檔案…" }));
    });
    const status = await screen.findByRole("status");
    await waitFor(() => expect(within(status).getByText("已加入音效庫")).toBeTruthy());
    expect(within(status).getByText("23.4 秒，超過 10 秒。已截斷成前 10 秒")).toBeTruthy();
    expect(within(status).getByText("超過 5 MB 上限")).toBeTruthy();
    // Unsupported files never reach the importer; the drop zone explains.
    expect(screen.getByText("這個我吃不下：.aiff 不在支援格式內")).toBeTruthy();
    await waitFor(() => expect(screen.getByText("慘叫")).toBeTruthy());
    expect(backend.data.sounds.some((s) => s.name === "long speech")).toBe(true);

    fireEvent.click(within(status).getAllByRole("button", { name: "移除這筆" })[0]);
    expect(within(screen.getByRole("status")).queryByText("已加入音效庫")).toBeNull();
  });

  it("imports files dropped on the window", async () => {
    const { backend } = renderWithBackend(<LibraryPage />);
    await screen.findByText("好痛 02");
    act(() => backend.data.dropFiles!({ type: "over", count: 2 }));
    expect(screen.getByText("放開就收下 2 個檔案")).toBeTruthy();
    await act(async () => backend.data.dropFiles!({ type: "drop", paths: ["/a/哎.wav", "/a/喔.flac"] }));
    await waitFor(() => expect(screen.getByText("喔")).toBeTruthy());
    expect(screen.getByText("把音效檔拖到這裡")).toBeTruthy();
  });

  it("renames and deletes a user sound in use", async () => {
    const { backend } = renderWithBackend(<LibraryPage />);
    fireEvent.doubleClick(await screen.findByText("好痛 02"));
    const input = screen.getByRole("textbox", { name: "重新命名音效" });
    fireEvent.change(input, { target: { value: "好痛 03" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText("好痛 03");

    // Built-in sounds can't be renamed.
    fireEvent.doubleClick(screen.getByText("喔！"));
    expect(screen.queryByRole("textbox", { name: "重新命名音效" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "好痛 03 的更多動作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "刪除…" }));
    const sheet = screen.getByRole("alertdialog");
    expect(sheet.textContent).toContain("「好痛 03」還在使用中");
    expect(within(sheet).getByText("辦公室安靜版")).toBeTruthy();
    fireEvent.click(within(sheet).getByRole("button", { name: "仍要刪除" }));
    await waitFor(() => expect(screen.queryByText("好痛 03")).toBeNull());
    const office = backend.data.profiles.find((p) => p.id === "user-office")!;
    expect(office.mapping.slap_hard?.sounds).toEqual(["builtin.scream"]);
  });

  it("built-in rows only offer preview", async () => {
    const { backend } = renderWithBackend(<LibraryPage />);
    await screen.findByText("喔！");
    fireEvent.click(screen.getByRole("button", { name: "喔！ 的更多動作" }));
    const items = screen.getAllByRole("menuitem").map((m) => m.textContent);
    expect(items).toEqual(["試聽"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "試聽" }));
    expect(backend.data.log).toContain("preview:builtin.oh");
  });

  it("draws waveform bars", () => {
    expect(wavePath([0, 1])).toBe("M1 9.4V10.6M3 2.0V18.0");
  });
});
