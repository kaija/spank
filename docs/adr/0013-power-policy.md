# ADR-0013：功耗策略 — Best effort + 睡眠/鎖定暫停

- 狀態：Accepted
- 日期：2026-10-24

## Decision

- 不設硬性功耗數字（best effort），但遵守以下基本規則：
  - 螢幕鎖定（`com.apple.screenIsLocked`）、系統睡眠、使用者按「暫停」時：App 通知 helper 停止讀取加速度計，角度輪詢停止。
  - Helper 沒有 client 連線時，自動關閉感測器 reporting。
  - 加速度計資料在 helper 內批次處理，不逐筆喚醒；只有「衝擊候選」才跨 IPC。
  - 設定視窗關閉即銷毀 WebView（ADR-0004）。
- 開發期用 `powermetrics` / Activity Monitor「能源影響」手動觀察，發現明顯耗電再優化。

## Consequences

- 沒有 CI 功耗回歸測試；靠手動檢查清單。

## 實作備註（2026-10-25，T42）

- ADR-0014 之後沒有 helper：上面「通知 helper」改由 App 內的 orchestrator 直接啟停感測器，規則不變（鎖定、睡眠、暫停時停止；加速度計每 ~50 ms 批次處理）。
- 實測（MacBook Air 15" M4，`spank-sensors` 的 `power-check` example，不含 UI 與音訊）：只有角度 0.18 % CPU、每秒 5 次喚醒；加上加速度計 0.78 % CPU、每秒約 20 次喚醒。音訊 stream 閒置時暫停（spike S5）。手動項目見 `docs/release-checklist.md`。
