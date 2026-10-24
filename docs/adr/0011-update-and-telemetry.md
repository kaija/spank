# ADR-0011：更新與遙測 — Tauri updater、無遙測

- 狀態：Accepted
- 日期：2026-10-24

## Decision

- 使用 **Tauri updater plugin**：簽章的 `latest.json` 放在 GitHub Releases。
- 每天檢查一次，有新版時詢問使用者再更新；可在設定關閉自動檢查。
- **不收集任何遙測**。當機 / 錯誤紀錄只寫本機
  `~/Library/Logs/<bundle-id>/`（`tracing` + 檔案輪替，保留 7 天）。
- 設定頁提供「複製診斷資訊」按鈕：機型、macOS 版本、感測器能力、最近 log（不含個人資料）。

## Consequences

- Updater 簽章私鑰需存 GitHub Secrets；遺失則無法推送更新。
- 沒有遙測，問題回報完全依賴使用者主動提供診斷資訊。
