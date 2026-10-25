# 參與貢獻

謝謝你想幫忙！最有幫助的兩件事是**其他機型的感測器 trace** 和**好聽的音效包**。程式碼也歡迎，請先讀下面的慣例。

## 開發環境

見 [README](README.md#開發環境)。送 PR 前請確認：

```bash
pnpm --dir apps/desktop build && pnpm --dir apps/desktop typecheck && pnpm --dir apps/desktop test
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

`pre-commit install` 會在 commit 時跑 fmt 與 clippy。

## 慣例

- 程式碼、註解、commit 訊息用英文；`docs/` 用繁體中文。
- 改動既有行為前先讀相關 ADR（`docs/adr/`）；新的架構決定請新增一份 ADR。
- 感測器參數來自實測（`docs/spike-report.md`），沒有新的量測資料不要改。
- `spank-core` 是純邏輯，不能有 `unsafe` 或作業系統相依；硬體與系統呼叫放在 `spank-sensors`、`spank-sysmon`。
- HID callback 是 `extern "C"`：裡面只能寫 buffer，不能 `println!`、`unwrap` 或配置記憶體（panic 會讓整個程式 abort）。
- UI 以 `mockup/` 為準；新字串要同時加進 `messages/zh-Hant.json` 與 `en.json`（測試會檢查兩者 key 一致）。
- 改了 `spank-core` 裡會傳給 UI 的型別後，重新產生 TypeScript 型別：`cargo test -p spank-core --features ts export_bindings`。

## 貢獻感測器 trace

拍打偵測（T23）目前只用一台 MacBook Air 15" M4、一個人的資料調過參數。你的機型錄一份 trace，就能幫忙確認別台 Mac 上也準確。

1. 在 MacBook 上錄製（要能打開、闔上螢幕，所以不能是闔蓋接外接螢幕的狀態）；程式會一步步提示你拍打、打字、開合螢幕等動作：

   ```bash
   cargo run --release -p trace-recorder -- --check          # 先確認兩個感測器都讀得到
   cargo run --release -p trace-recorder -- --scenario all --out fixtures/traces/<機型>-v1
   ```

   `<機型>` 例如 `m1pro14`、`m3air13`。`--list` 可看所有情境。
2. 看一下每段的峰值與簡易偵測結果：`python3 -I tools/analyze-trace.py fixtures/traces/<機型>-v1`。
3. 在 `fixtures/traces/<機型>-v1/expected.json` 標出每一下拍打的起始時間（格式同 `m4air-v1/expected.json`，從波形判讀）。
4. 跑 `cargo test -p spank-sensors --test slap_traces -- --nocapture`，把報表貼在 PR 裡。分數不到門檻也沒關係，那正是我們需要的資料。

Trace 只包含角度與加速度數值、機型代號（如 `Mac16,13`）與 macOS 版本，沒有其他個人資料。格式說明見 [`fixtures/traces/README.md`](fixtures/traces/README.md)。

## 貢獻音效包

音效包是 `.spankpack`（[ADR-0009](docs/adr/0009-spankpack-format.md)）：一個 ZIP，裡面有 `manifest.json` 與 48 kHz、已正規化的 WAV。

**最簡單的做法**：在 App 裡做。

1. 設定 › 音效庫：拖進你的音檔（WAV、MP3、M4A、AAC、FLAC、OGG）。App 會轉成 48 kHz、裁成 10 秒內、把響度正規化到 -16 LUFS。
2. 設定 › Profiles：新增一個 Profile，把音效拖到各事件上，試聽調整。
3. 把它設為使用中的 Profile，再按音效庫右上角「匯出…」（匯出的是使用中的 Profile），得到 `.spankpack`。

**授權規定**：

- 只收你自己錄製 / 製作的，或 CC0、CC-BY 授權的音效。CC-BY 請在 PR 裡寫出作者與來源網址。
- 不收電影、遊戲、卡通、名人等有版權的聲音，也不收 AI 模仿真人的聲音。
- 每個音效 ≤ 10 秒、≤ 5 MB；整包解壓後 ≤ 50 MB。

**想成為內建音效**：內建的三個 Profile 由 `tools/gen-builtin-sounds` 以程序合成產生（`assets/sounds/builtin/`），授權記在 `assets/sounds/SOUNDS_LICENSE.md`。要加入內建，請在 PR 中附上原始音檔、授權與來源，並更新 `SOUNDS_LICENSE.md`；內建總大小需維持在 5 MB 以下。

## 回報問題

請附上 設定 › 一般 ›「複製診斷資訊」的內容（只有機型、系統版本與感測器狀態），以及 `~/Library/Logs/dev.spank.app/spank.log` 中相關的段落（貼上前請確認沒有你不想公開的路徑）。

## 授權

貢獻的程式碼以 MIT 授權釋出；音效依其各自授權（見上）。
