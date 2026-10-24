# ADR-0006：拍打偵測 — 力道分級、連擊、誤觸抑制

- 狀態：Accepted（初始參數已由 Spike S2 trace 決定，見 [spike-report](../spike-report.md)；偵測改在 App 內執行，見 ADR-0014）
- 日期：2026-10-24

## Context

加速度計同時會收到：拍打、打字、觸控板按壓、放下筆電、螢幕開合、喇叭自身振動。
需要在不額外要求權限的前提下，把拍打分出來並分級。

## Decision

### 管線

1. **Sensor thread（App 內，ADR-0014）**：各軸高通濾波去重力 → 計算 |a| → 超過 `max(觸發門檻 0.2 g, k × 自適應 noise floor)` 時，擷取 80 ms 視窗特徵
   （`peak_g`、`energy`、`duration_ms`、`hf_ratio` 高頻能量比、`ts`）→ 送出 `ImpactCandidate`。
2. **App**：
   - **抑制閘（Suppression gates）**：以下期間丟棄候選
     - 螢幕角度正在變化中 + 結束後 500 ms
     - App 自己正在播放音效 + 結束後 200 ms
     - 打字：預設用訊號特徵（打字 = 小振幅、密集、`hf_ratio` 高；拍打 = 單一大峰值 + 低頻推動）；
       進階設定可開「精準模式」（需 Input Monitoring 權限，鍵盤/觸控板輸入後 300 ms 內丟棄）。
   - **分級**：依校準後的三個門檻分為 `Light` / `Medium` / `Hard`。
   - **連擊**：5 秒內第 3 次（含）以上 → `Combo(n)`，取代該次分級事件。
   - **Refractory**：觸發後 300 ms 內不再觸發（吸收 100–200 ms 的回彈峰值；實測連拍間隔 ~330 ms，故不可 > 300 ms）。

### 初始參數（Spike S2，M4 Air）

| 參數 | 值 | 依據 |
|------|----|------|
| 高通截止 | 2 Hz | 去重力與緩慢傾斜 |
| 觸發門檻 | 0.2 g | 打字 ≤ 0.083 g、觸控板 ≤ 0.087 g、拍打 ≥ 0.34 g |
| Refractory | 300 ms | 回彈峰值 + 連拍間隔 |
| 分級門檻 | 校準決定 | 實測輕 / 中幾乎相同（0.37 / 0.40 g），固定值不可靠 |

### 校準

首次啟用拍打時 30 秒校準：輕 / 中 / 重各拍一次，再打字數秒學習 noise floor。結果存 `calibration.json`，可重做。

## Consequences

- 需要真實 trace 才能調參；Spike S2 必須錄製拍打、打字、開合、放桌上、走路攜帶等場景。
- 外接螢幕暫停規則 **不** 預設啟用（使用者未選）。
- 單純峰值門檻已能過濾打字；訊號特徵濾波是次要保險，視更多 trace 決定是否需要。

## Alternatives

- **單一觸發**：太單調。
- **強制 Input Monitoring**：權限提示嚇人。
- **root helper 內判斷**：ADR-0014 已移除 helper。
