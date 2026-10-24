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
