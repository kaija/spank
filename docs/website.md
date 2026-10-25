# Creaky 產品網站與隱私政策

網站原始檔在 `website/`，是免安裝依賴、免建置的單頁 HTML/CSS。圖片為已核可廣告圖的壓縮 JPEG，全部自帶；沒有外部字型、分析工具、Cookie 或 JavaScript。

## 本機預覽

在 repo 根目錄執行：

```sh
python3 -m http.server 4173 --bind 127.0.0.1 --directory website
```

開啟 `http://127.0.0.1:4173`。

## 第一次部署到 GitHub Pages

1. 將 `website/`、`.github/workflows/pages.yml` 與本次文件變更 commit 並 push 到 `main`。
2. 在 `kaija/spank` 的 **Settings → Pages → Build and deployment → Source** 選 **GitHub Actions**。
3. 在 **Custom domain** 輸入 `creaky.arlo-ai.app` 並儲存。若介面尚未提供此欄位，可先完成一次部署。
4. 在 `arlo-ai.app` 的 DNS 新增 CNAME：名稱 `creaky`，目標 `kaija.github.io`。不要加 repo 名稱、`https://` 或路徑。
5. 到 **Actions → Creaky website → Run workflow** 選 `main`。之後 `main` 上的網站變更會自動部署。若第一次 push 發生在啟用 Pages 前，重新執行失敗的 workflow 即可。
6. 等待 DNS 驗證與憑證核發，在 Pages 勾選 **Enforce HTTPS**。確認 `https://creaky.arlo-ai.app/` 與以下錨點可公開存取。

`website/CNAME` 記錄預期網域，但 GitHub Actions 部署不會靠這個檔案設定網域；**仍須在 Settings → Pages 設定 Custom domain**。發布產物只包含 `website/`，不包含 repo 中的簽章、原始碼或其他檔案。

官方參考：[GitHub Pages 自訂 workflow](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)、[自訂網域](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/managing-a-custom-domain-for-your-github-pages-site)。

## App Store Connect 網址（網站正式上線後填入）

| 欄位 | 繁體中文與英文共用網址 |
| --- | --- |
| Marketing URL | `https://creaky.arlo-ai.app/` |
| Support URL | `https://creaky.arlo-ai.app/#support` |
| Privacy Policy URL | `https://creaky.arlo-ai.app/#privacy` |

英文隱私政策另可直達 `https://creaky.arlo-ai.app/#privacy-en`，不需要 JavaScript、登入或展開操作。

## 維護

- 頁面文字與完整政策：`website/index.html`；Markdown 版本：`docs/privacy.md`。修改政策時同步兩處與更新日期。
- 風格與手機版：`website/styles.css`。
- App 尚未公開上架，首頁目前標示「即將推出」。公開上架後，再換成正式下載連結 `https://apps.apple.com/app/id6826780064`。
- App Store 中英文介紹已移除開源描述；網站也不使用開源宣傳文案。
- 網站主機的安全紀錄與 App 的資料處理分開說明。依 [GitHub Pages 官方說明](https://docs.github.com/en/pages/getting-started-with-github-pages/what-is-github-pages)，GitHub 會為安全目的記錄訪客 IP。
