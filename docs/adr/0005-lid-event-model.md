# ADR-0005：螢幕開合 — 事件 + 角度區間模型

- 狀態：Accepted（`LidOverbent` 已由 ADR-0015 修訂為開啟中連續播放）
- 日期：2026-10-24

## Context

螢幕完全闔上時系統會睡眠，App 停止執行。所謂「打開螢幕」實際上是：
系統喚醒 → App 恢復 → 螢幕已經打開到某個角度。因此只能在醒著時根據角度變化觸發。

## Decision

定義三種離散事件，角度門檻可調，皆有 hysteresis 與 cooldown：

| 事件 | 觸發條件（預設值） | 範例音效 |
|------|-------------------|----------|
| `LidOpened` | 系統喚醒（`NSWorkspaceDidWakeNotification`）且當下角度 > `open_min`（10°） | 玻璃破碎 |
| `LidClosing` | 醒著時角度由上往下穿越 `close_threshold`（20°） | 低沉悶響 |
| `LidOverbent` | 角度由下往上穿越 `overbend_threshold`（120°） | 金屬斷裂 |

- Hysteresis：預設 5°，離開區間需超過門檻 ± hysteresis 才重新武裝（re-arm）。
- Cooldown：每個事件預設 3 秒。
- 取樣（Spike S1 結果）：主動推送的 input report 固定 ~1 Hz，太慢；改為 **輪詢 feature report**
  （`IOHIDDeviceGetReport(Feature, id=1)`，~0.7 ms／次）：閒置 5 Hz，偵測到變化後升到 30 Hz，持續 2 秒無變化降回。
- 角度範圍實測 14°～125°，解析度 1°。

## Consequences

- 不提供「連續吱嘎門聲」模式（未來可另開 ADR）。
- `LidClosing` 可能在螢幕闔上、系統睡眠前來不及播完 → 播放引擎需能在 ~300 ms 內出聲；接受偶爾被睡眠截斷。
- 外接螢幕的 clamshell 模式下，闔上不會睡眠，事件照常運作。

## Alternatives

- **連續門聲**：需持續音訊串流與高頻取樣，較耗電。
- **只在喚醒時播**：最簡單，但失去「掰過頭」的樂趣。
