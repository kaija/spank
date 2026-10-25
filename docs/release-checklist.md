# 發佈前檢查清單（T42）

每次發佈前在真機上走一遍。`[x]` 是 2026-10-25 已確認的項目（MacBook Air 15" M4，macOS 26.6.2）；`[ ]` 需要人在機器前操作。發佈本身的步驟見 [release.md](release.md)。

## 1. 自動檢查（CI，`.github/workflows/ci.yml`）

- [x] `cargo fmt --check`、`cargo clippy -D warnings`
- [x] `cargo test --workspace`（含 trace 重播 gate：precision / recall 門檻，報表在 job summary）
- [x] 覆蓋率 ≥ 80%（`cargo llvm-cov … --fail-under-lines 80`）
- [x] 前端 typecheck、build、Vitest
- [x] TypeScript bindings 與 Rust 型別一致
- [x] `cargo deny check advisories bans licenses sources`
- [ ] CI 在 GitHub 上綠燈（看 Actions 頁面）

## 2. 功耗（ADR-0013）

`cargo run --release -p spank-sensors --example power-check -- --secs 20`（App 的核心迴圈，不含 UI 與音訊；筆電靜置）：

| 狀態 | CPU | 喚醒 | 角度讀取 | 加速度計 |
|------|-----|------|----------|----------|
| 只有角度（拍打關） | 0.18 % | 5 次/秒 | 5 次/秒 | — |
| 角度 + 加速度計（拍打開） | 0.78 % | ~20 次/秒 | 5 次/秒 | 797 筆/秒 |

- [x] 上表量測結果（2026-10-25）
- [x] 音訊 stream 閒置時暫停，不讓 coreaudiod 常駐 ~12 % CPU（spike S5，`AudioPlayer`）
- [ ] 活動監視器 › 能源：App 閒置（設定視窗關閉）時「能源影響」接近 0；拍打開啟時沒有明顯升高
- [ ] 螢幕鎖定、睡眠、按「暫停」後感測器停止（設定 › 感測器的能力狀態、或 `sensor-check` 看不到資料）；解鎖 / 喚醒後恢復
- [ ] 設定視窗關閉後 WebView 被銷毀，Dock 圖示消失（ADR-0004）

## 3. 錯誤處理與 log

- [x] 執行期程式碼沒有可能 panic 的 `unwrap`（剩下的 `expect` 都是不變式：記憶體內 WAV、剛設定的視窗）
- [x] HID callback 只寫 ring buffer，不配置記憶體、不 panic；panic hook 會把加速度計 property 關掉
- [x] 設定檔毀損會備份成 `*.corrupt-<ms>` 並回到預設值（spank-store 測試）
- [x] 從 Finder 或登入時啟動，stderr 寫到 `~/Library/Logs/dev.spank.app/spank.log`（超過 1 MB 輪替成 `spank.log.1`）
- [ ] 從 Finder 打開 App，確認 log 檔出現 `---- spank started` 那一行；設定 › 一般 ›「打開 log 資料夾」打得開
- [ ] 拔掉 / 切換音訊輸出裝置後再觸發事件：下一次播放會重建 stream，不會沒聲音或卡住
- [ ] 「複製診斷資訊」貼出來的內容只有機型、系統版本、感測器 / 權限狀態與開關，沒有個人資料

## 4. Fuzz（`.spankpack` 是不受信任的輸入）

- [x] Stable 版替代測試跑一輪：4 個 seed × 250,000 個變異檔案，無 panic
  （`SPANK_FUZZ_ITERS=250000 SPANK_FUZZ_SEED=<n> cargo test --release -p spank-pack mutated_packs_never_panic`）
- [ ] `cargo +nightly fuzz run read_pack -- -max_total_time=600`（在 `crates/spank-pack/`；需要 nightly 與 `cargo install cargo-fuzz`）

## 5. 無障礙

- [x] 所有只有圖示的按鈕都有 `aria-label`；開關、滑桿、角度儀圓點有名稱與數值
- [x] 鍵盤：Tab 依序巡覽、⌘1–⌘5 切換設定頁、角度儀圓點用方向鍵（Shift 每次 5°）
- [x] 狀態變化（靜音原因、校準結果、匯入結果）用 `role="status"` 禮貌朗讀；連續變動的數值（角度）不朗讀
- [x] 「減少動態效果」：波形每 0.5 秒更新一次，取消位移 / 轉場動畫
- [ ] VoiceOver（⌘F5）走一次：Onboarding 6 步、感測器頁、自動靜音頁都能完成
- [ ] 淺色 / 深色模式各看一次，文字對比清楚
- [ ] 英文介面字串較長時會換行、不被截斷

## 6. 真機功能

- [ ] **T20 系統訊號**：`cargo run -p spank-sysmon --example sysmon-watch`，逐項切換麥克風、相機、音量、靜音、鎖定、睡眠，都有輸出
- [ ] **T31 Tray**：狀態列（運作中 / 已靜音：原因 / 已暫停）、暫停、切換 Profile、試聽 7 個事件、設定…、結束
- [ ] **T32 Profiles**：新增 / 複製 / 改名 / 刪除、事件對應拖放、試聽、啟用開關、音量
- [ ] **T33 音效庫**：拖放 / 選檔匯入（含太長、太大、格式錯誤）、波形、改名、刪除使用中的音效、匯入 / 匯出 `.spankpack`
- [ ] **T34 自動靜音**：四條規則、專注模式授權流程（需正式簽章版）、安靜時段跨午夜
- [ ] **T36 Onboarding**：刪掉 `~/Library/Application Support/dev.spank.app/config.json`（或用新使用者帳號）啟動，6 步走完；中途關閉後感測器頁顯示「尚未校準」
- [ ] **T36 校準**：輕 / 中 / 重各拍一下、打字 5 秒，完成後拍打會依力道哀叫；連拍觸發連擊
- [ ] **T37 感測器頁**：即時角度跟著螢幕動；拖曳門檻後開合在新角度觸發；靈敏度調高後輕拍更容易觸發
- [ ] **T38 一般頁**：登入時啟動（重新登入確認）、App 音量、語言切換後 Tray 也跟著換
- [ ] **T39 精準打字**：打開 → 系統詢問「輸入監控」→ 允許後大力打字不觸發拍打；拒絕時頁面提示，拍打照常
- [ ] **T41 更新**：發佈兩個版本後，裝舊版的 Mac 在「一般 › 檢查更新」看得到新版，更新後重新啟動成新版
- [ ] 其他機型（M1 / M2 / Pro）錄 trace 驗證 T23 參數（`trace-recorder --scenario all`）

## 7. 發佈

- [ ] [release.md](release.md) 的 Secrets 都設好
- [ ] tag 產出的 DMG 在乾淨機器上安裝、無 Gatekeeper 警告
