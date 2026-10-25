# spank

> 代號 `spank`，正式產品名稱待定（[ADR-0012](docs/adr/0012-licensing-and-naming.md)）。下文以「Spank」稱呼這個 App。

一個好玩的 macOS 選單列小工具：一台怕痛的 MacBook。

- 打開、闔上螢幕，或把螢幕往後掰過頭時，它會出聲。
- 拍它一下，它會依力道輕、中、重哀叫；短時間連續拍打會觸發「連擊」。
- 開會、專注模式、安靜時段、系統靜音時自動閉嘴。

所有偵測都在你的 Mac 上完成，不收集任何資料（[隱私說明](PRIVACY.md)）。用 Rust 和 Tauri v2 開發，平常只在選單列常駐。MIT 授權。

*A macOS menu bar toy: your MacBook squeaks when you open, close or over-bend the lid, and whines when you slap it (graded by strength, with combos). Everything runs locally; nothing is collected.*

## 支援的 Mac

- **Apple Silicon（M1 以上）的 MacBook Air / MacBook Pro**，macOS 14 以上。
- 需要內建的螢幕角度感測器與加速度計。App 啟動時會自動偵測，沒有的功能會在 設定 › 感測器 標示「不支援」，其他功能照常運作（例如沒有角度感測器時，拍打仍然可用）。
- 目前在 MacBook Air 15" M4 上實測。其他機型歡迎[貢獻 trace](CONTRIBUTING.md#貢獻感測器-trace) 幫忙驗證。
- 不支援：Intel Mac、桌上型 Mac（iMac、Mac mini、Mac Studio、Mac Pro）。

## 安裝

1. 從 [GitHub Releases](https://github.com/kaija/spank/releases) 下載最新的 `Spank_<版本>_aarch64.dmg`。
2. 打開 DMG，把 Spank 拖進「應用程式」。
3. 打開 Spank。App 經過 Apple 公證，不會出現「無法驗證開發者」的警告。
4. 第一次啟動會出現歡迎視窗：試試開合螢幕、選一個個性（毀滅、哀哀叫、經典），再決定要不要啟用拍打並花 30 秒校準手勁。

之後 Spank 住在選單列（右上角的小筆電圖示），不佔 Dock。從那裡可以暫停、換 Profile、試聽、打開設定。

更新：每天會檢查一次新版本，有新版時先問你再更新（設定 › 一般 可關閉，也可手動檢查）。

## Mac App Store 版（Creaky）

另有一個上架 Mac App Store 的版本 **Creaky**，免費，只有螢幕開合的部分：打開、慢慢打開時的連續音效、闔上，加上自動靜音、兩套內建音效與自訂音效。
App Store 版必須在 sandbox 裡執行，而 sandbox 不允許開啟加速度計，所以**拍打、連擊與校準只有 DMG 版 Spank 才有**（[ADR-0017](docs/adr/0017-mac-app-store-edition.md)）。
Creaky 不連網路，由 App Store 負責更新。兩版共用同一個 bundle ID，同一台 Mac 請擇一安裝。

上架流程、文案與審查備註見 [docs/app-store.md](docs/app-store.md)，隱私權政策見 [docs/privacy.md](docs/privacy.md)。

## 權限

**基本功能不需要任何權限，也不需要管理員密碼。** 以下兩個是選用的，只在你打開對應功能時才會詢問：

| 權限 | 用在 | 不給會怎樣 |
|------|------|------------|
| 專注模式狀態 | 自動靜音的「專注模式」規則 | 這條規則不生效，其他規則照常 |
| 輸入監控 | 精準打字模式：打字時暫停拍打偵測 | 仍會用震動特徵過濾打字，只是比較不精準 |

精準打字模式只記錄「有沒有按鍵」的時間，不會記錄按了哪個鍵。開會偵測只看麥克風 / 相機「是否被其他 App 使用」，不錄音、不錄影，也不需要麥克風或相機權限。

## 常見問題

**拍了沒反應？**
拍打預設是關的。到 設定 › 感測器 打開「拍打」並完成校準。也確認一下：選單列沒有顯示「已暫停」或「已靜音」、系統音量不是 0、目前的 Profile 對應的事件有音效且已啟用。

**打字會被當成拍打嗎？**
校準時的「打字 5 秒」會學你的打字震動，大部分情況會被過濾。很用力打字的話，打開「精準打字模式」（需要輸入監控權限）。

**拍桌子也會叫？**
會，這是刻意的：桌子震動傳到筆電，跟拍筆電的感覺很像（[ADR-0006](docs/adr/0006-slap-detection-model.md)）。

**太敏感 / 太遲鈍？**
設定 › 感測器 的「靈敏度」可以整體調整，或按「重新校準…」再量一次。螢幕角度的門檻可以直接拖曳角度儀上的圓點。

**會傷筆電嗎？**
輕拍就夠了。校準的「重拍」也請像拍桌子表達不滿那樣，不需要真的用力打。

**耗電嗎？**
很少。只開角度偵測時約 0.2% CPU，加上拍打偵測約 0.8%（M4 實測）；螢幕鎖定、睡眠或暫停時感測器會停止。

**為什麼 Dock 有時候出現 Spank？**
設定視窗或歡迎視窗開著的時候才會出現，關掉就消失。

**怎麼換成自己的音效？**
設定 › 音效庫 拖進音檔，再到 設定 › Profiles 複製一個 Profile 來改。也可以匯入別人分享的 `.spankpack` 音效包。

**介面語言？**
跟隨系統（繁體中文或英文），可在 設定 › 一般 切換。

**怎麼移除？**
選單列 › 結束，把 App 丟進垃圾桶，再刪除 `~/Library/Application Support/dev.spank.app/` 與 `~/Library/Logs/dev.spank.app/`。

**回報問題**：請開 [GitHub Issue](https://github.com/kaija/spank/issues)，附上 設定 › 一般 ›「複製診斷資訊」的內容。

## 文件

| 文件 | 內容 |
|------|------|
| [PRIVACY.md](PRIVACY.md) | 隱私說明 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 如何貢獻程式碼、感測器 trace、音效包 |
| [docs/adr/](docs/adr/README.md) | 架構決策紀錄 |
| [docs/design.md](docs/design.md) | 系統設計 |
| [docs/tasks.md](docs/tasks.md) | 依 Wave 排列的任務清單與目前進度 |
| [docs/spike-report.md](docs/spike-report.md) | 感測器、Focus、音訊的實測結果 |
| [docs/release.md](docs/release.md) | 簽章、公證與發佈流程 |
| [docs/release-checklist.md](docs/release-checklist.md) | 發佈前的真機檢查清單 |
| [docs/app-store.md](docs/app-store.md) | Mac App Store 版（Creaky）的上架流程與文案 |
| [docs/privacy.md](docs/privacy.md) | 隱私權政策（中英對照，App Store 使用） |

## 開發

狀態：Phase 1，Wave 0–7 實作完成，等待真機驗證與第一次發佈（T44）。

### 專案結構

```
crates/spank-core/      純邏輯：事件、偵測、校準、自動靜音、orchestrator（不依賴作業系統）
crates/spank-sensors/   螢幕角度、加速度計、鍵盤活動（IOKit），trace 格式與重播
crates/spank-sysmon/    睡眠、鎖定、麥克風 / 相機、音量、專注模式
crates/spank-audio/     音檔匯入（正規化）與播放
crates/spank-store/     設定、Profile、音效庫的檔案儲存
crates/spank-pack/      .spankpack 音效包讀寫與驗證
apps/desktop/           Tauri v2 app：src-tauri/（Rust），Next.js 靜態匯出（設定與歡迎視窗）
tools/                  感測器探測、trace 錄製與分析、內建音效產生、發佈腳本
fixtures/traces/        錄製的感測器資料（偵測回歸測試用）
```

### 環境

需要：Rust stable、Node 24、pnpm 11、Xcode Command Line Tools。

常用指令都包在 `Makefile` 裡，`make` 會列出全部：

| 指令 | 做什麼 |
|------|--------|
| `make setup` | 第一次：安裝 JS 套件、pre-commit hook、build 前端 |
| `make dev` | 開發模式（hot reload）並打開設定視窗 |
| `make ui` | 只在瀏覽器看設定 UI（假後端，不需要 Tauri） |
| `make app` / `make dmg` | 打包未簽章的 `.app`（/ DMG） |
| `make run` | 打開打包好的 `.app` |
| `make release` | Developer ID 簽章的 `.app` + DMG（見 `docs/release.md`） |
| `make check` | CI 會跑的檢查：lint、測試、bindings |

下面是各指令實際執行的內容。

```bash
pnpm install
```

```bash
pre-commit install
```

Tauri 會把前端打包進執行檔，所以第一次 build Rust 前要先 build 前端：

```bash
pnpm --dir apps/desktop build
```

**開發模式**（Next.js dev server 加 Tauri，並打開設定視窗）：

```bash
pnpm --dir apps/desktop tauri dev -- -- --open-settings
```

只看 UI 也可以 `pnpm --dir apps/desktop dev`，用瀏覽器開 http://localhost:3000：沒有 Tauri 時會用假的後端（含模擬波形與校準）。

**打包**（輸出到 `target/release/bundle/macos/Spank.app`）：

```bash
pnpm --dir apps/desktop tauri build --bundles app
```

**測試與檢查**：

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
pnpm --dir apps/desktop typecheck && pnpm --dir apps/desktop test
```

### 感測器工具

都不需要 root，要在真的 MacBook 上跑。

```bash
cargo run --release -p spank-sensors --example sensor-check -- --slaps
```

```bash
cargo run --release -p spank-sensors --example power-check
```

```bash
cargo run --release -p trace-recorder -- --scenario all --out fixtures/traces/<機型>-v1
```

更多指令見 [CLAUDE.md](CLAUDE.md#commands)。

## 授權

程式碼：MIT，見 [LICENSE](LICENSE)。內建音效（「全玻璃」與「地城石門」）全部是作者以 ElevenLabs 產生、只授權隨 Spank 使用，不屬於 CC0 或 MIT。詳見 [assets/sounds/SOUNDS_LICENSE.md](assets/sounds/SOUNDS_LICENSE.md)。
