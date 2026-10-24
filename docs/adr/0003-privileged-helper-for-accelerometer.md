# ADR-0003：權限分離 — 以 root helper 讀取加速度計

- 狀態：**Superseded by [ADR-0014](0014-in-app-accelerometer.md)**（Spike S2 證實不需 root；本文件保留為未來 macOS 收緊權限時的備案）
- 日期：2026-10-24

## Context

既有開源專案（例如 Go 版 spank、apple-silicon-accelerometer）讀取 SPU 加速度計時，
需要先對 `AppleSPUHIDDriver` service 設定 `SensorPropertyReportingState` / `SensorPropertyPowerState` /
`ReportInterval` 等 registry property，這一步通常需要 root。

GUI 整個以 root 執行會擴大攻擊面，也無法正常啟動 / 發佈。

## Decision

- 將加速度計讀取拆成獨立的 **`spank-helper` LaunchDaemon**，以 `SMAppService.daemon(plistName:)` 註冊，嵌在 `.app/Contents/Library/LaunchDaemons/`。
- Helper **只做三件事**：開啟加速度計、做特徵擷取（偵測「衝擊候選」）、透過 IPC 送出事件。不讀使用者設定、不碰檔案、不解析外部輸入。
- 分級、連擊、抑制、播放等邏輯全部在一般權限的主 App。
- IPC：Unix domain socket。Helper 以 `LOCAL_PEERTOKEN`（audit token）取得連線端，並用 `SecCode` 驗證其簽章符合本 App 的 Team ID + bundle ID，不符就斷線。
- 使用者首次啟用拍打功能時才註冊 helper（ADR-0006 / onboarding），需至「系統設定 → 一般 → 登入項目」手動核准。

## Consequences

- 拍打功能需要使用者輸入管理員密碼 + 手動核准，onboarding 必須清楚引導。
- 需處理 helper 與 App 的版本相容：IPC 協定帶 `protocol_version`，不符時 App 重新註冊 helper。
- 移除 App 時需提供「移除 helper」選項（`SMAppService.unregister`）。
- 若 spike 證明 **不需要 root**：本 ADR 改為 Superseded，加速度計直接在主 App 讀取，架構大幅簡化。

## Alternatives

- **整個 App sudo 執行**：不可發佈。
- **麥克風偵測敲擊**：需麥克風權限、常駐耗電、環境噪音誤判。
- **XPC Mach service**：Rust 端 XPC 綁定不成熟；Unix socket + 簽章驗證足夠。
