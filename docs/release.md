# 發佈流程（T40）

`.github/workflows/release.yml` 在推送 `v*` tag 時執行，產出可在乾淨機器上安裝、沒有 Gatekeeper 警告的 DMG，並建立 **草稿** GitHub Release（確認後手動發佈）。

## 流程

1. Tag 必須等於 workspace 版本（`Cargo.toml` 的 `version`，例如 `0.1.0` → `v0.1.0`）。
2. 跑一次前端 build / Vitest 與 `cargo test --workspace`。
3. 匯入 Developer ID 憑證到暫時的 keychain；還原 provisioning profile 與 notary API key。
4. `tools/release.sh --notarize`：
   - `tauri build --bundles app --config src-tauri/tauri.release.conf.json`：Developer ID 簽章、Hardened Runtime、release entitlements（`apps/desktop/src-tauri/Spank.entitlements`）、內嵌 `Contents/embedded.provisionprofile`。
   - 驗證簽章、entitlements、profile、hardened runtime。
   - `notarytool submit --wait` → `stapler staple`（.app）。
   - 有 `TAURI_SIGNING_PRIVATE_KEY` 時：把（已 staple 的）.app 打包成 `Spank_<版本>_aarch64.app.tar.gz`、用 updater 私鑰簽出 `.sig`，並以 `tools/latest-json.py` 產生 `latest.json`（T41）。
   - `hdiutil` 建 DMG（含 `/Applications` 捷徑）→ 簽章 → 公證 → staple → `spctl` 檢查。
5. `gh release create --draft`，附上 DMG、`.sha256`、updater 的 `.app.tar.gz`／`.sig` 與 `latest.json`。
6. 不論成功與否都刪除 keychain、profile 與 API key。

只出 `aarch64-apple-darwin`（ADR-0001）。

### 自動更新（T41）

- App 讀 `https://github.com/kaija/spank/releases/latest/download/latest.json`（`tauri.conf.json` 的 `plugins.updater.endpoints`），用同一處的 `pubkey` 驗簽。草稿 Release 不算 latest，**發佈後**舊版才看得到新版。
- Release build 啟動 2 分鐘後檢查一次，之後每 24 小時一次（設定 › 一般 › 自動檢查更新可關）；有新版時跳原生對話框詢問，同意才下載、安裝並重新啟動。「稍後」在這次執行期間不再問同一版。一般頁也可手動檢查與安裝。開發版不會自己檢查。
- Updater 金鑰：私鑰在 `~/.tauri/spank-updater.key`（不在 repo 內，**遺失就無法再發更新給已安裝的使用者**，請另外備份），公鑰已寫進 `tauri.conf.json`。換金鑰要同時換 `pubkey`，舊版使用者會收不到之後的更新。

### 為什麼 entitlements 只在 release 用

`com.apple.developer.usernotifications.communication` 是受限 entitlement：沒有對應 provisioning profile 的 .app 會被 AMFI 拒絕啟動（"No matching profile found"）。開發版（`tauri dev` / 一般 `tauri build`）不帶這些 entitlements，所以專注模式狀態讀不到（設定頁會顯示「需要正式簽章的版本」），其餘功能正常。見 spike-report S4。

## GitHub Secrets

| Secret | 內容 | 取得方式 |
|--------|------|----------|
| `APPLE_CERTIFICATE` | Developer ID Application 憑證 + 私鑰的 `.p12`，base64 | 鑰匙圈存取 › 「Developer ID Application: Chang KaiChieh (H2ZM466J6A)」› 輸出 `.p12`；`base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | 上面 `.p12` 的密碼 | 輸出時設定 |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Chang KaiChieh (H2ZM466J6A)` | `security find-identity -v -p codesigning` |
| `KEYCHAIN_PASSWORD` | CI 暫時 keychain 的密碼 | 任意隨機字串 |
| `MACOS_PROVISION_PROFILE` | `Spank Developer ID` provisioning profile（App ID `H2ZM466J6A.dev.spank.app`，含 Communication Notifications），base64 | developer.apple.com › Profiles；本機在 gitignored 的 `signing/Spank_Developer_ID.provisionprofile`；`base64 -i signing/Spank_Developer_ID.provisionprofile \| pbcopy` |
| `NOTARY_KEY_ID` | App Store Connect API key 的 Key ID | App Store Connect › 使用者與存取 › 整合 › App Store Connect API（角色 Developer 即可） |
| `NOTARY_ISSUER` | 同頁的 Issuer ID | 同上 |
| `NOTARY_KEY_P8` | 下載的 `AuthKey_<KeyID>.p8`，base64 | `base64 -i AuthKey_XXXX.p8 \| pbcopy`（只能下載一次） |
| `TAURI_SIGNING_PRIVATE_KEY` | Updater 私鑰（檔案內容） | `pbcopy < ~/.tauri/spank-updater.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Updater 私鑰密碼（目前為空字串，可不設） | 產生時設定 |

`gh secret set <NAME>` 可從終端設定（值從 stdin 讀）。

注意：provisioning profile 綁定 bundle ID 與憑證。ADR-0012 決定正式產品名稱 / bundle ID 後要重新註冊 App ID、重發 profile，並更新 `Spank.entitlements`、`tauri.conf.json` 的 `identifier` 與這個 secret。憑證更新後 profile 也要重發。

## 本機試跑

不公證（只驗證簽章與 entitlements，產出的 DMG 在別台 Mac 會被 Gatekeeper 警告）：

```bash
APPLE_SIGNING_IDENTITY="Developer ID Application: Chang KaiChieh (H2ZM466J6A)" tools/release.sh
```

完整公證（需要 API key）：

```bash
APPLE_SIGNING_IDENTITY="Developer ID Application: Chang KaiChieh (H2ZM466J6A)" \
NOTARY_KEY_ID=… NOTARY_ISSUER=… NOTARY_KEY_PATH=~/keys/AuthKey_XXXX.p8 \
tools/release.sh --notarize
```

加上 updater 產物：再設 `TAURI_SIGNING_PRIVATE_KEY=~/.tauri/spank-updater.key`。

輸出在 `target/release-dist/`。profile 預設讀 `signing/Spank_Developer_ID.provisionprofile`，可用 `SPANK_PROVISION_PROFILE` 指定其他路徑。

## 發佈前檢查

- [ ] 在沒有開發工具的乾淨帳號（或另一台 Mac）下載 DMG、拖進「應用程式」、打開：沒有「無法驗證開發者」警告。
- [ ] `spctl --assess --type execute -vv /Applications/Spank.app` 顯示 `source=Notarized Developer ID`。
- [ ] 首次啟動出現 Onboarding；專注模式規則能讀到狀態（開勿擾 → 靜音）。
- [ ] 發佈後，裝著上一版的 Mac 在「設定 › 一般 › 檢查更新」看得到新版，更新後重新啟動成新版。
- [ ] 確認後把草稿 Release 發佈。
