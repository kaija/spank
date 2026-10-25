# ADR-0008：音訊 — 匯入正規化、單聲部優先權中斷

- 狀態：Accepted
- 日期：2026-10-24

## Decision

### 匯入

- 接受 `wav` / `mp3` / `m4a` / `aac` / `flac` / `ogg`（`symphonia` 解碼）。
- 限制：檔案 ≤ 5 MB、處理後長度 ≤ 10 秒（超過則截斷並提示）。
- 處理流程：解碼 → mono / stereo 保留，超過 2 聲道混為 mono（避免丟失中置聲道）→ 重新取樣到 48 kHz（`rubato`）→ 去除前導靜音（< -50 dBFS）
  → 響度正規化到 **-16 LUFS**（`ebur128`），並限制 true peak ≤ -1 dBTP → 存為 48 kHz f32/16-bit WAV。
- 存放：`~/Library/Application Support/<bundle-id>/sounds/<uuid>.wav` + `sounds/index.json`（名稱、來源、長度、sha256、授權）。
- 原始檔不保留。

### 播放

- 使用 `cpal`，輸出到系統預設裝置。
- **Stream 生命週期（Spike S5）**：啟動時建立 output stream 並保持 **paused**（常駐輸出會讓 coreaudiod 吃 ~12% CPU）；
  觸發時 `play()`（實測 20–40 ms），播完 500 ms 後 `pause()`；預設輸出裝置變更時重建。
- 目前 Profile 用到的音效 **預先解碼到記憶體**，觸發延遲目標 < 50 ms。
- **單聲部 + 優先權中斷**：`Combo > Hard > Overbent > Medium > Opened > Closing > Light`。
  高優先權事件以 50 ms fade-out 中斷目前音效；同級或較低則丟棄。

### 內建音效

- 只用 **CC0 / 公有領域** 音源（Freesound CC0、Sonniss GDC 免費包中 CC0 部分等），來源記錄於 `SOUNDS_LICENSE.md`。
- 內建 3 個 Profile：「毀滅」（玻璃 / 金屬）、「哀哀叫」（喔 / 尖叫）、「經典」（吱嘎 / 悶響）；每事件 2–3 個，總計 < 5 MB。

## Consequences

- 不支援錄音（不需麥克風權限）。
- 音量一致，使用者體驗穩定；但原始檔被取代，無法「還原」。

## 修訂（2026-10-25）

- 「內建音效只用 CC0」放寬：「經典」的開合音效（石門、木扣喀噠、刺耳摩擦、木頭裂開）改用專案作者以 ElevenLabs Sound Effects 產生的音效，授權記為 `LicenseRef-Spank-ElevenLabs`：只隨 Spank 散布，不屬於 CC0 或 MIT。其餘內建音效仍是程序合成的 CC0（`tools/gen-builtin-sounds`）。
- 原因：使用者試聽後認為合成的開合摩擦聲太假（見 `docs/sound-use-cases.md`），偏好實際音色。
- 原檔放在 `assets/sounds/source/`，產生器轉 mono、限幅（最多 12 dB）、正規化後打包；`SOUNDS_LICENSE.md` 逐一列出來源。
- 「毀滅」的開啟中音效（30–120°，ADR-0015）改用三個作者以 ElevenLabs 產生的「玻璃緊繃碎裂」錄音（各約 7 秒，`LicenseRef-Spank-ElevenLabs`），取代合成的金屬斷裂／鋼筋彎折；開啟中會連續播放、停下暫停，需要較長的音檔。喚醒與拍打仍用合成的短玻璃聲。
- 內建音效總量上限由 5 MB 放寬為 **8 MB**（解壓後的 WAV 總和；`spank-pack` 測試與產生器都檢查），以容納長錄音。
