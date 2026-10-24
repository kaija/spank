# ADR-0004：UI — Tauri v2 + Next.js 靜態匯出

- 狀態：Accepted
- 日期：2026-10-24

## Context

核心邏輯用 Rust（低功耗、可直接呼叫 IOKit / CoreAudio）。需要一個設定視窗：
Profile 編輯、拖放上傳、波形預覽、校準精靈。App 大部分時間只在選單列常駐。

## Decision

- **Tauri v2** 作為 App 殼層：tray（`TrayIconBuilder`）、視窗管理、updater、IPC command。
- 設定視窗前端用 **Next.js（`output: "export"` 靜態匯出）+ TypeScript**，i18n 用 `next-intl`（zh-Hant / en）。
- **關閉設定視窗即銷毀 WebView**，閒置時只剩 Rust 主程序 + tray。
- `LSUIElement = true`：無 Dock 圖示；開啟設定視窗時切換 `ActivationPolicy::Regular`，關閉後切回 `Accessory`。

## Consequences

- 閒置記憶體目標 ~15–25 MB，CPU 近零。
- Next.js 只能用靜態功能（無 SSR / API routes / `next/image` 最佳化）。
- 前端與 Rust 透過 Tauri `invoke` / event 溝通，型別需同步（考慮 `specta` / `tauri-specta` 產生 TS 型別）。

## Alternatives

- **objc2 + AppKit 純原生**：最省，但 UI 開發極慢。
- **egui / eframe**：快寫但非原生外觀。
- **Slint**：生態較小、tray 需另配。
