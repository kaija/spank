# spank

> 代號 `spank`，正式產品名稱待定（[ADR-0012](docs/adr/0012-licensing-and-naming.md)）。

一個好玩的 macOS 選單列小工具：
- 打開、闔上 MacBook 螢幕，或把螢幕往後掰過頭時，會播放音效。
- 拍打筆電時，它會依力道哀叫，連續拍打會觸發「連擊」。

用 Rust 和 Tauri v2 開發，平常只在選單列常駐。

- 支援：Apple Silicon MacBook（M1 以上），macOS 14 以上
- 狀態：Phase 1 開發中。Wave 0–1 已完成，其中 T04（Focus）只完成一部分，剩下的併入 T09

## 文件

| 文件 | 內容 |
|------|------|
| [docs/adr/](docs/adr/README.md) | 架構決策紀錄 |
| [docs/design.md](docs/design.md) | 系統設計 |
| [docs/tasks.md](docs/tasks.md) | 依 Wave 排列的任務清單（同一個 Wave 內可平行） |
| [docs/spike-report.md](docs/spike-report.md) | 感測器、Focus、音訊 spike 的結果 |
| [docs/ui-design-prompt.md](docs/ui-design-prompt.md) | 給 Claude Design 用的 UI 設計 prompt |

## 專案結構

```
crates/spank-core/      純邏輯：事件、Profile、設定（不依賴作業系統）
apps/desktop/           Tauri v2 app：src-tauri/（Rust），Next.js 靜態匯出（設定視窗）
tools/                  spike 與開發工具：lid-probe、accel-probe、audio-probe、focus-probe、錄製與分析腳本
fixtures/traces/        錄製的感測器資料（給重播測試用）
```

## 開發環境

需要：Rust stable、Node 24、pnpm 11、Xcode Command Line Tools。

```bash
pnpm install
```

```bash
pre-commit install
```

**開發模式**（Next.js dev server 加 Tauri，設定視窗會連到 localhost:3000）：

```bash
pnpm --dir apps/desktop tauri dev -- -- --open-settings
```

**打包**（輸出到 target/release/bundle/macos/Spank.app）：

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

### 感測器工具

不需要 root。

```bash
cargo run -p lid-probe -- --mode poll --secs 10 --interval-ms 50
```

```bash
cargo run -p accel-probe -- --enable --secs 5
```

錄製一段感測器 trace（會依提示請你拍打、開合螢幕）：

```bash
tools/record-spike.sh
```

## 授權

MIT，見 [LICENSE](LICENSE)。內建音效的授權會另外列在 `SOUNDS_LICENSE.md`（T35）。
