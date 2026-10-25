# ADR-0017：Mac App Store 版「Creaky」— 只有螢幕開合

- 狀態：Accepted
- 日期：2026-10-25
- 修訂：[ADR-0002](0002-distribution-dmg-not-app-store.md)（DMG 仍是完整版；另外多一個 App Store 版）

## Context

ADR-0002 決定只發 DMG，理由之一是「sandbox 內能否開啟感測器未經驗證」。ADR-0014 之後 root 已不是阻礙，
所以在這台 MacBook Air 15" M4（macOS 26.6）上做了兩個 sandbox 實驗，用 Developer ID 簽章、開啟 App Sandbox 的 `.app`
（`tools/run-as-app.sh` 經 LaunchServices 啟動）：

1. **螢幕角度感測器：可以。** 加上 `com.apple.security.device.usb` 後，sandbox 內可以開啟 `05AC:8104` 的角度 HID 裝置，
   並 poll feature report（`GetReport(Feature, id=1)`），讀值與未 sandbox 時相同。沒有這個 entitlement 時開啟失敗。
2. **加速度計：不行。**
   - 正常路徑是對 `AppleSPUHIDDriver` 寫 `SensorPropertyReportingState`、`SensorPropertyPowerState`、`ReportInterval`
     （ADR-0014）。Sandbox 禁止寫 IORegistry property（`IORegistryEntrySetCFProperty` 被拒），加速度計不會開始推送。
   - 改走 HID 裝置介面（`IOHIDDeviceSetProperty` 設同樣的 property）：**連未 sandbox 都收到 0 筆報告**。
     這些 property 只在 driver 的 registry entry 上有效，HID 裝置本身沒有 feature element 可以用來開啟推送。
   - 也就是說，sandbox 內沒有已知的公開或半公開方式能讓加速度計開始送資料。

所以 App Store 版只能做螢幕開合；拍打、校準與精準打字（都依賴加速度計）只留在 DMG 版。

## Decision

- **兩個版本，同一份程式碼：**

  | | DMG 版「Spank」 | App Store 版「Creaky」 |
  |---|---|---|
  | 散布 | Developer ID + 公證 DMG（ADR-0002） | Mac App Store，免費 |
  | Sandbox | 不啟用 | 啟用（App Store 必要） |
  | 螢幕開合（打開、開啟中 30–120°、闔上） | ✅ | ✅ |
  | 拍打、連擊、校準、精準打字 | ✅ | ❌ |
  | 自動靜音、兩套內建 Profile（全玻璃、地城石門）、自訂音效 | ✅ | ✅ |
  | 更新 | Tauri updater（ADR-0011） | App Store 自己處理，不內建 updater |
  | 登入時啟動 | autostart plugin | `SMAppService.mainApp` |
  | 網路 | 只有檢查更新 | 完全沒有 |

- **名稱 Creaky**（App Store 上的顯示名稱）。DMG 版仍叫 Spank；ADR-0012 的正式命名問題對 DMG 版仍未定案。
- **兩版共用 bundle ID `dev.spank.app`**：同一個 App ID（`H2ZM466J6A.dev.spank.app`，已有 Communication Notifications），
  只是多一份 Mac App Store provisioning profile。
  - 同一台 Mac 不應同時裝兩版；兩版是同一個 App 的兩種散布方式，不是兩個產品。
  - Sandbox 版的資料在 `~/Library/Containers/dev.spank.app/`，不會讀到 DMG 版的 `~/Library/Application Support/dev.spank.app/`。
- **以 Cargo feature 切換**，在 `spank-desktop`：
  - `direct`（預設）：DMG 版。含加速度計、拍打、校準、輸入監控、updater、autostart plugin。
  - `appstore`：App Store 版。不編入加速度計、輸入監控與 updater 相關程式碼（不只是隱藏 UI），
    登入時啟動改用 `SMAppService`，產品名稱、entitlements 與 profile 換成 App Store 版的。
  - 兩個 feature 互斥；`spank-core` 等函式庫 crate 不需要知道版本，由 app 殼決定要建立哪些 sensor 與指令。
  - 前端依後端回報的能力（`capabilities()` 已經在做）隱藏拍打相關頁面與 onboarding 步驟，不另外維護一份 UI。
- **App Store 版的 entitlements**：
  - `com.apple.security.app-sandbox`；
  - `com.apple.security.device.usb`：角度感測器是 HID 裝置（實驗 1）；
  - `com.apple.security.files.user-selected.read-write`：用檔案對話框匯入音檔、匯入／匯出 `.spankpack`（`tauri-plugin-dialog`）；
  - `com.apple.developer.usernotifications.communication`：專注模式狀態（spike-report S4）；
  - profile 帶的 `com.apple.application-identifier` / `com.apple.developer.team-identifier`。
  - `com.apple.security.network.client`：WKWebView 在沙盒裡必須有它。WebKit 的網路行程連 App 內建的介面都要經手，
    沒有它會不斷當掉、每個視窗一片空白（本機沙盒測試實際遇到）。App 本身不發出任何網路請求。
  - 不要 `network.server`、麥克風、相機或 Apple Events。
- Info.plist 加 `ITSAppUsesNonExemptEncryption = false`（不用加密，也不發出網路請求），
  `LSApplicationCategoryType` 改為 `public.app-category.entertainment`。
- **不使用 temporary-exception entitlements**（見 Alternatives）。
- App Store 版關閉 Tauri 的 `macOSPrivateApi`（`tauri.conf.json` 目前是 `true`），App Review 會拒絕使用私有 API 的 App。

## Consequences

- 多一條發佈流程：`tools/release-appstore.sh` 建 `.pkg` 並上傳 App Store Connect，步驟、上架文案與審查備註見
  [`docs/app-store.md`](../app-store.md)；隱私政策見 [`docs/privacy.md`](../privacy.md)。
- 需要 Apple Distribution 與 Mac Installer Distribution 憑證、Mac App Store provisioning profile，以及 App Manager 角色的
  App Store Connect API key。
- 每次改到感測器、sandbox 或 entitlements 都要兩版各測一次；CI 至少要讓 `--features appstore` 能 build 與跑測試。
- 角度感測器仍是未公開的 HID 裝置（Guideline 2.5.1 的風險沒有消失），只是改以公開的 IOKit HID API 存取；
  審查備註要說明 `device.usb` 的用途。被拒時 DMG 版不受影響。
- macOS 更新若連 sandbox 內的角度感測器也鎖住，App Store 版會失去唯一的感測器；`capabilities()` 會回報不支援，
  App 只能顯示說明。
- App Store 版沒有 updater 也沒有網路，PRIVACY.md 的「網路」一節只適用 DMG 版。

## Alternatives

- **維持 ADR-0002，不上 App Store**：最省事，但放棄 App Store 的曝光；而螢幕開合本身已經夠好玩。
- **用 temporary-exception entitlement 讓加速度計可用**（例如 `com.apple.security.temporary-exception.iokit-user-client-class`
  開 `IOHIDLibUserClient` / `AppleSPUHIDDriverUserClient`，或 `temporary-exception.sbpl` 開 IORegistry 寫入）：
  - App Review 對 temporary exception 要逐項說明理由，未公開 driver 的 property 寫入幾乎不可能通過；
  - 實驗 2 顯示 HID 裝置介面本身收不到資料，必須寫 driver 的 registry property，等於要求 sandbox 開一個大洞；
  - Apple 文件說 temporary exception 隨時可能不再被接受，送審通過也不代表下一版能過。
- **App Store 版帶 privileged helper 開加速度計**（ADR-0003）：Guideline 2.4.5 禁止要求 root，必退。
- **App Store 版用獨立 bundle ID（例如 `dev.spank.creaky`）**：兩版可以並存，但要再註冊一個 App ID、申請 Communication
  Notifications、維護兩套 profile；兩版本來就不該同時跑（會同時對開合出聲），共用比較單純。
- **分兩個 repo 或長期分支**：程式碼 90% 相同，用 feature 切換維護成本最低。
