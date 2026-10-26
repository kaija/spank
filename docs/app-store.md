# Mac App Store 上架流程（Creaky）

App Store 版叫 **Creaky**，只有螢幕開合（打開、開啟中 30–120°、闔上）、自動靜音和兩套內建 Profile；
拍打、校準與精準打字只在 DMG 版 Spank。為什麼這樣切見 [ADR-0017](adr/0017-mac-app-store-edition.md)，DMG 版的流程見 [release.md](release.md)。

| 項目 | 值 |
|------|----|
| 名稱 | Creaky |
| Bundle ID | `dev.spank.app`（與 DMG 版共用，App ID `H2ZM466J6A.dev.spank.app`） |
| SKU | `creaky-mac` |
| 價格 | 免費，所有地區 |
| 主要類別 | 娛樂（Entertainment） |
| Cargo feature | `appstore`（DMG 版是預設的 `direct`） |
| 最低系統 | macOS 14，只有 Apple Silicon（arm64） |
| 支援網址 | `https://creaky.arlo-ai.app/#support` |
| 隱私權政策網址 | `https://creaky.arlo-ai.app/privacy.html` |

網站首頁為 `https://creaky.arlo-ai.app/`，支援網址為 `https://creaky.arlo-ai.app/#support`，獨立隱私權政策網址為 `https://creaky.arlo-ai.app/privacy.html`。部署方式見 [網站部署說明](website.md)。

## 1. 一次性準備（依序）

### 1.1 憑證

在 developer.apple.com › Certificates 建立（已經有就跳過），裝進「鑰匙圈存取」：

- **Apple Distribution**：簽 `.app`。
- **Mac Installer Distribution**：簽上傳用的 `.pkg`（鑰匙圈裡顯示為 `3rd Party Mac Developer Installer: Chang KaiChieh (H2ZM466J6A)`）。

確認：

```bash
security find-identity -v
```

### 1.2 Provisioning profile

1. developer.apple.com › Identifiers › `dev.spank.app`：確認 **Communication Notifications** 已勾選（DMG 版的 profile 已經用到，通常不用改）。
2. Profiles › 新增 › Distribution › **Mac App Store Connect**。
3. App ID 選 `H2ZM466J6A.dev.spank.app`，憑證選上面的 Apple Distribution，名稱填 `Creaky App Store`。
4. 下載後存成 **`signing/Creaky_App_Store.provisionprofile`**（`signing/` 在 gitignore 內，**不要 commit**）。

憑證更新後 profile 也要重發。

### 1.3 App Store Connect 的 App 記錄

App Store Connect › App › ＋ › 新增 App：

- 平台：**macOS**
- 名稱：**Creaky**
- 主要語言：繁體中文（台灣）。另外加英文（美國）在地化。
- 套裝識別碼：`dev.spank.app`
- SKU：**`creaky-mac`**
- 使用者存取權限：完整存取

名稱在 App Store 是全球唯一的；被占用時改用 `Creaky – Lid Sounds` 之類，App 內的顯示名稱不必跟著改。

### 1.4 App Store Connect API key

App Store Connect › 使用者與存取 › 整合 › App Store Connect API › 團隊金鑰 › ＋：

- 名稱：`creaky-upload`
- 角色：**App 管理**（App Manager）。DMG 版公證用的 Developer 角色金鑰不能上傳 App。
- 下載 `AuthKey_<KeyID>.p8`（只能下載一次），放到 **`~/.appstoreconnect/private_keys/`**（`altool` 預設會在這裡找）：

```bash
mkdir -p ~/.appstoreconnect/private_keys && mv ~/Downloads/AuthKey_*.p8 ~/.appstoreconnect/private_keys/ && chmod 600 ~/.appstoreconnect/private_keys/AuthKey_*.p8
```

記下同一頁的 **Key ID** 與 **Issuer ID**，上傳時要用。

## 2. Build、測試、上傳

`tools/release-appstore.sh` 用 `--features appstore` 建 Creaky.app：Apple Distribution 簽章、App Sandbox、
App Store 版 entitlements、內嵌 `signing/Creaky_App_Store.provisionprofile`，再用 `productbuild` 包成以
Mac Installer Distribution 簽章的 `.pkg`。實際的環境變數與參數以腳本開頭的說明為準。

每次上傳的 build 號（`CFBundleVersion`）都要比上一次大；版本號（`CFBundleShortVersionString`）沿用 workspace 版本。

**本機測試 sandbox 版**（不上傳）。App Store 簽章的 App 在本機打不開，所以這個模式改用可在本機執行的簽章，
但保留 sandbox 與同一組 entitlements，用來確認角度感測器、自動靜音與匯入音檔在 sandbox 內正常：

```bash
tools/release-appstore.sh --local-test
```

**建 `.pkg` 並上傳 App Store Connect**：

```bash
ASC_KEY_ID=XXXXXXXXXX ASC_ISSUER_ID=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx tools/release-appstore.sh --upload
```

不加 `--upload` 只會產出 `.pkg` 並驗證簽章。上傳後約 10–30 分鐘 build 會出現在 App Store Connect（處理完會寄信）。

**送審前用 TestFlight 試一次**：App Store Connect › TestFlight › 把這個 build 加到內部測試群組，
在 Mac 上用 TestFlight App 安裝。這是唯一能在本機跑「真正 App Store 簽章」版本的方式。

### 送審前的真機檢查

- [ ] 首次啟動出現歡迎視窗，沒有拍打、校準相關的步驟。
- [ ] 慢慢打開螢幕：30–120° 之間持續出聲，停手就停，繼續打開接著播；闔上低於門檻時有聲音；從睡眠打開時有聲音。
- [ ] 選單列切換 Profile（全玻璃、地城石門、竹筷）、暫停、試聽都正常。
- [ ] 設定頁沒有拍打、感測器校準、精準打字、檢查更新等項目。
- [ ] 自動靜音：開視訊會議（麥克風使用中）、開勿擾、系統靜音時不出聲。
- [ ] 設定 › 音效庫 用檔案對話框和拖曳都能匯入音檔；`.spankpack` 能匯入與匯出。
- [ ] 設定 › 一般 › 登入時啟動：打開後出現在 系統設定 › 一般 › 登入項目，登出再登入會自動啟動。
- [ ] `codesign -d --entitlements - Creaky.app` 有 `network.client`（WebKit 必須，否則視窗空白），沒有 `network.server`。

## 3. App Store Connect 的設定

### 3.1 App 資訊

- 類別：主要 **娛樂**，次要 不選（或「工具程式」）。
- 內容版權：選「是，包含第三方內容，且我有權使用」。內建音效是作者以 ElevenLabs 產生、授權隨本 App 散布（`assets/sounds/SOUNDS_LICENSE.md`）。
- 年齡分級：見 §5。
- 隱私權政策網址：`https://creaky.arlo-ai.app/privacy.html`

### 3.2 價格與供應狀況

免費；所有國家或地區。

### 3.3 版本資訊

每個語言貼 §4 的文案。另外需要：

- **截圖**：至少 1 張、最多 10 張，16:10，`2880×1800`（或 `2560×1600`、`1440×900`、`1280×800`）。建議：選單列下拉選單、Profiles 設定頁、歡迎視窗的螢幕開合示範、自動靜音設定頁。
  只截 App 的視窗（`⌘⇧4` 再按空白鍵選視窗），不要截到其他私人視窗。
- 版權：`2026 Chang KaiChieh`
- 建置版本：選剛上傳的 build。
- 出口合規：Info.plist 已有 `ITSAppUsesNonExemptEncryption = false`，不會再問；若有問，答「否，不使用加密」。

### 3.4 送審

App 審查資訊 › 備註貼 §6；聯絡資訊填自己的。不需要登入資訊（取消勾選「需要登入」）。
存檔後按 **加入審查** › **提交送審**。選「手動發布」可以在通過後自己決定上架時間。

## 4. 上架文案

限制：名稱 ≤ 30、副標題 ≤ 30、宣傳文字 ≤ 170、描述 ≤ 4000、關鍵字 ≤ 100 字元（逗號分隔、逗號後不加空白、不重複 App 名稱）。
以下文案都已在限制內。

### 繁體中文（台灣）

**名稱**

```
Creaky
```

**副標題**

```
開蓋嘎吱、闔上喀啦的筆電音效
```

**宣傳文字**

```
讓你的 Mac 也有個性！搞怪、可愛，還是中二熱血？換上你喜歡的音效，讓每次開蓋都像你的專屬登場。平凡的日常，也能很好玩。
```

**描述**

```
Creaky 讓你的筆電螢幕變成一扇會出聲的門。

慢慢掀開螢幕，它會跟著嘎吱作響：手停下，聲音就停；繼續打開，聲音接著播。闔上時喀啦一聲，從睡眠中打開時也會打聲招呼。

兩套成套設計的音效
• 全玻璃：整台都是玻璃，打開時慢慢裂開，闔上時清脆一裂。
• 地城石門：開蓋像推開沉重的石門，砂石落下，闔上時轟然停住。
也可以匯入自己的音檔，做一套屬於你的 Profile。

該安靜的時候會安靜
• 視訊會議中（有其他 App 正在使用麥克風或相機）
• 專注模式開啟時
• 你設定的安靜時段
• 系統靜音或音量為 0
Creaky 只檢查麥克風「是否被其他 App 使用」，從不錄音，也不需要麥克風權限。

輕巧、私密
• 只住在選單列，不佔 Dock
• 不需要帳號，也不連網路
• 不收集任何資料，所有偵測都在你的 Mac 上完成
• 螢幕鎖定或睡眠時停止偵測，幾乎不耗電

需求
需要內建螢幕角度感測器的 Apple Silicon MacBook Air 或 MacBook Pro（M1 以上），macOS 14 以上。此版本不支援桌上型 Mac 與 Intel 機型。
```

**關鍵字**

```
螢幕,開蓋,闔上,音效,嘎吱,惡搞,好玩,整人,選單列,玻璃,石門,地城,筆電,聲音,趣味,鉸鏈
```

**此版本新增內容**

```
Creaky 第一版：開合螢幕時出聲，內建「全玻璃」與「地城石門」兩套音效，會議、專注模式與安靜時段自動靜音。
```

**支援網址**

```
https://creaky.arlo-ai.app/#support
```

### English (U.S.)

**Name**

```
Creaky
```

**Subtitle**

```
Creaks when you open the lid
```

**Promotional Text**

```
Give your Mac a little main-character energy. Go cute, quirky, or gloriously dramatic—pick your own sounds and make every lid opening your signature entrance.
```

**Description**

```
Creaky turns your laptop lid into a door that talks back.

Open the lid slowly and it creaks along with you: stop, and the sound stops; keep going, and it picks up where it left off. Close it and it clacks shut. It even says hello when it wakes from sleep.

Two matching sound sets
• All Glass: the whole laptop is made of glass. It cracks slowly as it opens and snaps crisply when it closes.
• Dungeon Gate: opening the lid grinds open a heavy stone gate, grit trickling down, and closing it lands with a solid thud.
You can also import your own audio files and build a profile of your own.

Quiet when it should be
• During video calls (when another app is using the microphone or camera)
• When a Focus is on
• During quiet hours you set
• When the Mac is muted or the volume is 0
Creaky only checks whether another app is using the microphone. It never records audio and doesn't need microphone permission.

Light and private
• Lives in the menu bar, not the Dock
• No account, no network access
• Collects no data; everything happens on your Mac
• Sensing stops while the screen is locked or asleep, so it barely uses any power

Requirements
An Apple Silicon MacBook Air or MacBook Pro (M1 or later) with a built-in lid angle sensor, running macOS 14 or later. Desktop Macs and Intel models are not supported by this release.
```

**Keywords**

```
lid,laptop,sound effects,creak,squeak,hinge,funny,prank,toy,menu bar,glass,dungeon,door,open,close
```

**What's New**

```
First release of Creaky: sounds when you open and close the lid, two built-in sound sets (All Glass and Dungeon Gate), and auto-mute for meetings, Focus and quiet hours.
```

**Support URL**

```
https://creaky.arlo-ai.app/#support
```

## 5. App 隱私與年齡分級

### App 隱私（App Store Connect › App 隱私）

- 「你或你的第三方合作夥伴是否會從此 App 收集資料？」→ **否，我們不會從此 App 收集資料**。
- 結果顯示為 **Data Not Collected（未收集資料）**。
- 理由（備查）：App 不發出任何網路請求（`network.client` 只是 WebKit 顯示內建介面所需）、沒有分析或廣告 SDK、沒有帳號；設定與匯入的音效只存在 App 的 sandbox container。
  麥克風 / 相機只檢查「是否被其他 App 使用中」，不擷取任何音訊或影像。

### 年齡分級

問卷每一題都答「無」或「否」：

- 暴力、性、粗話、恐怖、賭博、藥物、醫療資訊等內容：**無**。玻璃碎裂與石門的聲音是音效，不是暴力內容。
- 無限制網頁存取：**否**。
- 使用者產生內容、與其他使用者通訊：**否**（匯入的音檔只在自己的 Mac 上播放，不會分享給其他人）。
- 家長監護、年齡驗證：不適用。

預期結果：**4+**。

## 6. App 審查備註

貼在 App 審查資訊 › 備註（英文，審查團隊看英文）：

```
Creaky plays sound effects when the MacBook lid is opened and closed. It is a menu bar app (no Dock icon); after launch, look for the small laptop icon at the right of the menu bar.

HARDWARE REQUIRED
Creaky needs a MacBook with a built-in lid angle sensor: an Apple Silicon MacBook Air or MacBook Pro (M1 or later; the sensor first appeared in 2019 MacBook models). Desktop Macs and Intel Macs have no such sensor; on them Creaky shows a "not supported" notice.

HOW TO TEST
1. Launch Creaky. A welcome window opens and demonstrates the lid sound.
2. Open and close the lid slowly. Between about 30 and 120 degrees a continuous creaking sound plays; it pauses when the lid stops moving and resumes when it moves again.
3. Keep closing the lid below about 20 degrees to hear the closing sound. Opening the lid after sleep plays a wake sound.
4. From the menu bar icon you can switch between the two built-in profiles (All Glass, Dungeon Gate), pause, and preview sounds. Settings are in the same menu.
If you hear nothing: check that the Mac is not muted, no Focus is on, and no other app (e.g. a video call) is using the microphone or camera. Creaky mutes itself in those cases on purpose.

ENTITLEMENTS
- com.apple.security.device.usb: the MacBook's built-in lid angle sensor is a HID device. Creaky opens it with the public IOKit HID API, read-only, to read the lid angle. It does not access any other USB or HID device, and it does not read the keyboard or trackpad.
- com.apple.security.files.user-selected.read-write: import the user's own audio files and import/export sound packs through the standard open/save panels.
- com.apple.developer.usernotifications.communication: used only to read whether a Focus is on (INFocusStatusCenter), so Creaky can stay silent. Creaky does not send notifications or messages; INSendMessageIntent is listed in NSUserActivityTypes only because App Store Connect requires it with this entitlement (ITMS-90894).
- com.apple.security.network.client: required by WKWebView in the App Sandbox. The settings window is a web view of files bundled in the app, and WebKit's network process needs this entitlement to load them; without it the window stays blank. Creaky itself makes no network requests.

PRIVACY
No account or sign-in. Creaky makes no network requests (the network.client entitlement is only for WebKit, see above). No data is collected or transmitted. Creaky never records audio or video: it only checks whether another app is currently using the microphone or camera (CoreAudio / CoreMediaIO device state), to stay silent during calls, so it does not request microphone or camera permission.
```

## 7. 通過之後

- 送審被拒：在 App Store Connect 的「解決方案中心」回覆。最可能被問的是 `device.usb` 的用途（§6 已說明）與只支援有角度感測器的 MacBook。
- 發新版：版本資訊 › ＋ 新版本，提高版本號，重跑 `tools/release-appstore.sh --upload`，填「此版本新增內容」後送審。
- DMG 版（Spank）照 [release.md](release.md) 另外發，兩版各自有版本紀錄，但共用 workspace 版本號。
