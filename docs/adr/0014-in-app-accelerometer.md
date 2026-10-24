# ADR-0014：加速度計直接在 App 內讀取（不需 root）

- 狀態：Accepted（前提：T09 在簽章 + Hardened Runtime 的 .app 內驗證通過）
- 日期：2026-10-24
- 取代：[ADR-0003](0003-privileged-helper-for-accelerometer.md)

## Context

Spike S2（[spike-report](../spike-report.md)）在 M4 / macOS 26.6 上證實：
一般使用者（uid=501）即可對 `AppleSPUHIDDriver` 設定 `SensorPropertyReportingState`、`SensorPropertyPowerState`、
`ReportInterval`，並開啟加速度計 HID 裝置接收 ~796 Hz 的報告。

## Decision

- **移除 `spank-helper` 與 IPC 層**。`IoHidAccel` 直接在 App 程序內執行，跑在專用的 sensor thread（自己的 CFRunLoop）。
- 生命週期：啟用拍打時設定 property 並開始讀取；暫停 / 鎖定 / 睡眠 / 結束時把 property 設回 0（停止推送、省電）。
- 異常結束（crash）時 property 可能維持開啟 → 下次啟動先重設，並在 `Drop` + panic hook 中盡力還原。
- 感測器 callback 內 **不可 panic**（`extern "C"` 中 panic 會 abort）：callback 只把資料推進 lock-free ring buffer，處理在另一個 thread。
- 啟動時做 capability 檢查：若 property 設定回傳 `kIOReturnNotPrivileged` 或收不到報告，拍打功能標示「此系統不支援」，並記錄診斷資訊。
- ADR-0003 的 helper 設計保留為 **備案**，若未來 macOS 收緊權限再啟用。

## Consequences

- 不需要管理員密碼、不需要到系統設定核准登入項目 → onboarding 大幅簡化（校準精靈仍保留）。
- 少了 helper 簽章、版本相容、IPC 安全性等工作（原任務 T25、T29 移除）。
- 依賴未公開行為，macOS 更新可能失效 → 每個 macOS 大版本需重跑 probe。
- 重新評估 App Store 的可行性變得有意義（root 不再是阻礙），但 sandbox 是否允許仍未知；ADR-0002 維持不變。
