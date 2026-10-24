# ADR-0001：目標平台 — Apple Silicon MacBook、macOS 14+

- 狀態：Accepted
- 日期：2026-10-24

## Context

App 依賴兩個筆電內建感測器：

- **螢幕角度感測器**：HID `0x05AC:0x8104`，UsagePage `0x20`（Sensor）/ Usage `0x8A`（Orientation），Transport `SPU`。
- **加速度計**：同一 SPU 裝置，vendor UsagePage `0xFF00` / Usage `3`，未公開文件。

開發機（MacBook Air 15" M4，`Mac16,13`，macOS 26.6）兩者皆存在（`hidutil list` 確認）。
Intel Mac 沒有 SPU，部分舊款 M1/M2 機型角度感測器讀不到。

## Decision

- 支援 **Apple Silicon（M1 以上）MacBook Air / Pro**，最低 **macOS 14 (Sonoma)**。
- 啟動時做 **capability detection**：逐一探測感測器，不存在或讀不到就把對應功能關掉並在 UI 標示「此機型不支援」。
- 不支援 Intel、桌機（Mac mini / Studio / iMac 無這些感測器）。

## Consequences

- 需要一份「已驗證機型」清單，靠社群回報擴充。
- 功能模組必須能各自獨立啟用／停用（lid-only、slap-only、both）。
- macOS 14 底線讓我們可以用 `SMAppService`（13+）與較新的 CoreAudio API。

## Alternatives

- **只支援開發機**：最快，但不利發佈。
- **含 Intel**：需另走 SMC Sudden Motion Sensor，僅少數舊機有，成本高、體驗不一致。
