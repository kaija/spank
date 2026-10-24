# ADR-0012：授權與命名 — MIT 開源、spank 為代號

- 狀態：Accepted
- 日期：2026-10-24

## Decision

- 程式碼以 **MIT** 授權開源，公開 repo，使用 **個人 GitHub 帳號**（`git-user personal`）。
- 內建音效授權另列於 `SOUNDS_LICENSE.md`。
- `spank` 僅為 **專案代號**（repo、crate 前綴）。對外產品名稱、bundle ID 尚未決定，
  集中在一個設定處（`app.config.toml` / `tauri.conf.json` 的 productName、identifier），可一次替換。
- 暫用 bundle ID：`dev.spank.app`（helper：`dev.spank.helper`）。

## Consequences

- 正式命名前不要在程式碼中硬編碼產品名稱字串；UI 文案透過 i18n key 取得。
- 「spank」英文含性暗示、且已有同名開源專案，正式發佈前需改名。
