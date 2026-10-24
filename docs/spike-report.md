# Spike 報告

- 機器：MacBook Air 15" M4（`Mac16,13`），macOS 26.6.2（25G83），Rust 1.93.0
- 日期：2026-10-24
- 工具：`tools/lid-probe`、`tools/accel-probe`、`tools/record-spike.sh`、`tools/analyze-spike.py`
- 錄製資料：`fixtures/traces/spike-m4air-01/`（accel 54k 樣本、lid、labels、lid-poll）

## 結論摘要

| Spike | 問題 | 結果 |
|-------|------|------|
| S1 | 角度感測器能否非 root 讀取？怎麼讀？ | ✅ 可以。**必須輪詢 feature report**（主動推送固定 1 Hz，太慢） |
| S2a | 加速度計是否需要 root？ | ✅ **不需要**（uid=501 可設定 driver property 並讀取） |
| S2b | 報告格式 / 取樣率 | ✅ 22 bytes，x/y/z int32 LE @ 6/10/14，÷65536 = g，~796 Hz |
| S2c | 拍打可否與打字等區分？ | ✅ 單一峰值門檻即可（見下表） |
| S3 | 在簽章 + Hardened Runtime 的 .app 內是否同樣可行？ | ⏳ 待驗證（T09，改為「不含 helper」版本） |
| S4 | `INFocusStatusCenter` 可由 objc2 呼叫？ | ⏳ 待驗證（T04） |
| S5 | cpal 常駐 stream 閒置耗電 | ⏳ 待驗證（T05） |

## S1 — 螢幕角度感測器

- 裝置：`05AC:8104`，UsagePage `0x20` / Usage `0x8A`，Transport `SPU`，MaxInputReportSize 8、MaxFeatureReportSize 1、ReportInterval 8000 µs。
- `IOHIDManagerOpen` / `IOHIDDeviceOpen`：一般權限成功。
- 報告格式：3 bytes `[0x01, angle_lo, angle_hi]`，u16 LE，單位「度」，解析度 1°。
- **Input report（主動推送）**：固定約 **1 Hz**，螢幕移動中也一樣 → 不能用來偵測開合事件。
- **Feature report 輪詢**（`IOHIDDeviceGetReport(Feature, id=1)`）：即時反映角度；50 ms 間隔下 10 秒內 52 次數值變化（107° → 53°），中位延遲 ~0.7 ms，185 次 0 失敗。
- 觀察到的範圍：14°（幾乎闔上）～ 125°（最大）。

→ **決策**：輪詢 feature report。閒置 5 Hz，偵測到變化後升到 30 Hz，持續 2 秒無變化降回。

## S2 — 加速度計

- Driver：`AppleSPUHIDDriver`（UsagePage `0xFF00` / Usage `3`），預設 `ReportInterval=0`，`SensorPropertyReportingState` / `PowerState` 不存在（= 關閉）。
- **一般權限**下 `IORegistryEntrySetCFProperty` 設定 `SensorPropertyReportingState=1`、`SensorPropertyPowerState=1`、`ReportInterval=1000` 皆回傳 `kIOReturnSuccess`。
- 未設定 property 時開啟裝置收不到任何報告；設定後立即開始推送。結束時設回 0 可停止推送（已驗證）。
- 報告：22 bytes；bytes 0–1 為遞增序號；x/y/z 為 int32 LE @ offset 6/10/14，÷65536 = g。
- 取樣率：**~796 Hz**（`ReportInterval=1000` µs 時）。
- 靜置：|a| ≈ 0.995 g，sd ≈ 0.0005 g。

### 拍打 vs 其他動作（高通 2 Hz 後的 |a|，單位 g）

| 片段 | p99 | 最大值 | 0.2 g / 300 ms 偵測到的事件 |
|------|-----|-------|---------------------------|
| 靜置 | 0.078 | 0.19（按 Enter 後的餘震） | 0 |
| 打字 | 0.051 | 0.083 | 0 |
| 觸控板 | 0.032 | 0.087 | 0 |
| 輕拍 ×3 | 0.24 | 0.38 | 3（0.37 / 0.38 / 0.36） |
| 中拍 ×3 | 0.29 | 0.41 | 3（0.39 / 0.40 / 0.41） |
| 重拍 ×3 | 0.43 | 0.78 | 4（0.78 / 0.57 / 0.63 / 0.68）— 使用者可能拍了 4 次 |
| 連拍 ×5 | 0.30 | 0.39 | 5（間隔 312–352 ms） |
| 螢幕開合 | 0.025 | 0.12 | 0 |

參數掃描：門檻 0.2–0.25 g、refractory 200–300 ms 都能得到正確數量；refractory 400 ms 會漏掉連拍。

### 觀察

- 每次拍打後約 100–200 ms 有 0.1–0.2 g 的回彈峰值 → refractory ≥ 200 ms 可吸收。
- 拍打與打字 / 觸控板之間有 **~4 倍** 的餘裕；單純峰值門檻就足以過濾打字，訊號特徵濾波是次要保險。
- 這次「輕」與「中」力道幾乎相同（0.37 vs 0.40）→ 分級必須靠個人校準，不能用固定值。
- 只有一台機器、一位使用者、一段錄製；放桌上、走路攜帶、喇叭播放等情境尚未錄製（T10）。

## 對 ADR / 設計的影響

1. **ADR-0003 → Superseded by ADR-0014**：加速度計直接在 App 內讀取，不需要 root helper。
2. **ADR-0005**：取樣方式改為 feature report 輪詢（5 Hz / 30 Hz 自適應）。
3. **ADR-0006**：初始參數 = 高通 2 Hz、觸發門檻 0.2 g、refractory 300 ms、同級 cooldown 由 400 ms 改為不另設（refractory 已涵蓋）；連擊窗 5 s 不變。
4. **ADR-0002（備註）**：root 已不是上 App Store 的阻礙，但 sandbox 內能否存取 HID / 設定 registry property 仍未知，維持 DMG 決策。
5. **tasks.md**：移除 helper / IPC 相關任務（原 T25、T29），T09 改為驗證簽章 .app 內的感測器存取。

## 風險

- 非 root 可設定 SPU driver property 可能是 macOS 26 的行為；未來 macOS 更新若收緊，需回退到 ADR-0003 的 helper 方案 → 保留 ADR-0003 文件作為備案。
- 需要在 M1/M2/M3 與 MacBook Pro 上重跑 probe（社群回報）。
