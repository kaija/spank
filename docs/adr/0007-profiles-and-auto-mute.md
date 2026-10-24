# ADR-0007：情境 — Profiles + 自動靜音規則

- 狀態：Accepted
- 日期：2026-10-24

## Context

需求「使用者可以依照情境設定他喜歡的音效」。

## Decision

### Profile

- 一個 Profile = 完整的 **事件 → 音效清單** 對應：
  `LidOpened`、`LidClosing`、`LidOverbent`、`SlapLight`、`SlapMedium`、`SlapHard`、`SlapCombo`。
- 每個事件可掛多個音效，播放時隨機挑選（避免連續重複同一個）。
- 每個事件可單獨停用；每個 Profile 有自己的音量倍率。
- 從選單列手動切換 Profile；內建 3 個（見 ADR-0008），使用者可複製、新增、刪除自訂 Profile。

### 自動靜音規則（各自可開關，預設全開）

| 規則 | 偵測方式 | 權限 |
|------|---------|------|
| 開會中 | CoreAudio `kAudioDevicePropertyDeviceIsRunningSomewhere`（麥克風）+ CoreMediaIO `kCMIODevicePropertyDeviceIsRunningSomewhere`（相機），排除本 App | 無 |
| 專注模式 | `INFocusStatusCenter.isFocused` | Focus 狀態權限 |
| 安靜時段 | 使用者定義時段（可多段，跨午夜） | 無 |
| 跟隨系統靜音 | 預設輸出裝置的 mute / volume；最終音量 = 系統音量 × Profile 音量 × App 音量 | 無 |

任一規則成立 → 靜音（事件照常偵測、不播放）。選單列圖示顯示靜音狀態與原因。

## Consequences

- 不做「依條件自動切換 Profile」的規則引擎（未來可另開 ADR）。
- `INFocusStatusCenter` 需從 Rust 透過 objc2 呼叫，並需 `NSFocusStatusUsageDescription`；Spike 時確認 objc2 綁定可用性。

## Alternatives

- 規則引擎自動切換：UI 與邏輯複雜。
- 只有事件對應、無 Profile：彈性不足。
