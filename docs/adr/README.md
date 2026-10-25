# Architecture Decision Records

格式：簡化版 MADR（Context / Decision / Consequences / Alternatives）。
狀態：`Proposed`（待 spike 驗證）→ `Accepted` → `Superseded by ADR-xxxx`。

| ADR | 標題 | 狀態 |
|-----|------|------|
| [0001](0001-target-platform.md) | 目標平台：Apple Silicon MacBook、macOS 14+ | Accepted |
| [0002](0002-distribution-dmg-not-app-store.md) | 發佈：Developer ID 公證 DMG，不上 App Store | Accepted |
| [0003](0003-privileged-helper-for-accelerometer.md) | 權限分離：以 root helper 讀取加速度計 | Superseded by 0014（保留為備案） |
| [0004](0004-ui-stack-tauri-nextjs.md) | UI：Tauri v2 + Next.js 靜態匯出 | Accepted |
| [0005](0005-lid-event-model.md) | 螢幕開合：事件 + 角度區間模型 | Accepted |
| [0006](0006-slap-detection-model.md) | 拍打偵測：力道分級、連擊、誤觸抑制 | Accepted |
| [0007](0007-profiles-and-auto-mute.md) | 情境：Profiles + 自動靜音規則 | Accepted |
| [0008](0008-audio-pipeline.md) | 音訊：匯入正規化、單聲部優先權中斷 | Accepted（內建音效一節由 0016 取代） |
| [0009](0009-spankpack-format.md) | 音效包格式 `.spankpack`（Phase 2 介面） | Accepted |
| [0010](0010-testing-sensor-replay.md) | 測試：感測器抽象 + trace 重播 | Accepted |
| [0011](0011-update-and-telemetry.md) | 更新與遙測：Tauri updater、無遙測 | Accepted |
| [0012](0012-licensing-and-naming.md) | 授權與命名：MIT 開源、spank 為代號 | Accepted |
| [0013](0013-power-policy.md) | 功耗策略：Best effort + 睡眠/鎖定暫停 | Accepted |
| [0014](0014-in-app-accelerometer.md) | 加速度計直接在 App 內讀取（不需 root） | Accepted |
| [0015](0015-continuous-opening-sound.md) | 開啟中（30–120°）連續播放，停下暫停、繼續續播 | Accepted |
| [0016](0016-elevenlabs-builtin-sounds.md) | 內建音效：以 ElevenLabs 產生、同一材質成套設計 | Accepted |
