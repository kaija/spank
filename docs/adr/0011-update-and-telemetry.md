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

## 實作備註（2026-10-25，T38 / T41）

- Updater：`apps/desktop/src-tauri/src/update.rs`。Release build 啟動 2 分鐘後檢查、之後每 24 小時；原生對話框詢問，「稍後」在同一次執行中不再問同一版。私鑰在 `~/.tauri/spank-updater.key`（CI 用 Secret `TAURI_SIGNING_PRIVATE_KEY`），公鑰在 `tauri.conf.json`。見 `docs/release.md`。
- Log：沒有引入 `tracing`。未接終端機時把 stderr 導到 `~/Library/Logs/dev.spank.app/spank.log`，超過 1 MB 輪替成 `spank.log.1`（只留一份），而不是保留 7 天。
- 「複製診斷資訊」**不含 log**：log 可能有使用者匯入的檔案路徑。改為提供「打開 log 資料夾」，由使用者自己挑要貼的段落。
