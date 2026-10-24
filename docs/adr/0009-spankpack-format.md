# ADR-0009：音效包格式 `.spankpack`（Phase 2 介面）

- 狀態：Accepted
- 日期：2026-10-24

## Context

Phase 2 將有自建後端讓使用者上傳 / 分享音效（帳號、審核、後端技術選型另開 grilling）。
Phase 1 先定下交換格式，讓匯入 / 匯出可用，Phase 2 直接沿用。

## Decision

`.spankpack` = ZIP 檔，內容：

```
manifest.json
sounds/<file>.wav        # 已正規化（ADR-0008）
icon.png                 # 選用，256x256
```

`manifest.json`（`schema_version: 1`）：

```json
{
  "schema_version": 1,
  "id": "com.example.glass-pack",
  "name": { "en": "Glass Pack", "zh-Hant": "玻璃包" },
  "version": "1.0.0",
  "author": "someone",
  "license": "CC0-1.0",
  "sounds": [
    { "file": "sounds/shatter1.wav", "sha256": "…", "duration_ms": 1200,
      "suggested_events": ["LidOpened"], "license": "CC0-1.0", "source_url": "…" }
  ],
  "profile": { "LidOpened": ["sounds/shatter1.wav"] }
}
```

- `license` 使用 SPDX identifier；每個音效可覆寫。
- 匯入時：驗證 schema、sha256、單檔 ≤ 5 MB / ≤ 10 s、ZIP 總解壓大小 ≤ 50 MB、拒絕路徑穿越（`..`、絕對路徑、symlink）。
- `profile` 選填；有則匯入時建立對應 Profile。
- App 內預留 `PackSource` trait（`LocalFile` 先實作；Phase 2 加 `RemoteCatalog`）與帳號模組掛點。

## Consequences

- 未來要改格式需升 `schema_version` 並保留舊版讀取。
- 匯入器是處理不受信任輸入的地方，需 fuzz 測試。
