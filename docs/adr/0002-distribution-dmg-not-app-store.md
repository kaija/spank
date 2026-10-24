# ADR-0002：發佈 — Developer ID 公證 DMG，不上 App Store

- 狀態：Accepted
- 日期：2026-10-24

## Context

原本希望上 Mac App Store，但：

1. **App Review Guideline 2.4.5(iv)**：Mac App Store app 不得要求提升到 root 權限 → 與 ADR-0003 的 privileged helper 衝突。
2. **Guideline 2.5.1**：僅能使用公開 API → 加速度計位於未公開的 vendor HID page。
3. App Store app 必須 sandbox；sandbox 內能否開啟角度感測器 HID 裝置未經驗證。

## Decision

- 只發佈 **Developer ID 簽章 + Apple 公證（notarized）的 DMG**，放在 GitHub Releases，由 GitHub Actions 產出。
- **不上 App Store**，也不維護 sandbox 版本。
- App 不啟用 App Sandbox，但啟用 **Hardened Runtime**（公證必要）。

## Consequences

- 需要付費 Apple Developer Program 帳號（$99/年）與 CI 中的簽章憑證 / notarytool 憑證（存 GitHub Secrets）。
- 自動更新需自行處理（見 ADR-0011）。
- 失去 App Store 曝光；行銷靠 GitHub / 社群。

## 備註（2026-10-24 Spike 後）

Spike S2 證實加速度計不需 root（ADR-0014），第 1 點阻礙已消失。
第 2、3 點（未公開裝置、sandbox 內 HID / registry property 存取）仍未驗證，維持 DMG 決策；
若未來想上 App Store，先做一個 sandbox spike。

## Alternatives

- **雙版本**（App Store lite + DMG full）：維護成本高，且 lite 版能否通過 sandbox 未知。
- **App Store only、捨棄拍打**：失去核心樂趣。
- **硬送審**：root helper 幾乎必退。
