# ADR-0010：測試 — 感測器抽象 + trace 重播

- 狀態：Accepted
- 日期：2026-10-24

## Context

GitHub Actions macOS runner 是虛擬機，沒有角度感測器與加速度計。

## Decision

- 感測器包在 trait 後：

  ```rust
  pub trait LidSource { fn next(&mut self) -> Option<LidSample>; }      // angle_deg, ts
  pub trait AccelSource { fn next(&mut self) -> Option<AccelSample>; }  // x, y, z (g), ts
  ```

  實作：`IoHidLid`、`IoHidAccel`（真機）、`TraceReplay`（測試）、`Synthetic`（單元測試）。
- **Trace 格式**：NDJSON，首行 header（機型、macOS 版本、取樣率、標註），之後每行一筆樣本；
  另附 `*.labels.json` 標註區段（`slap_light` / `slap_hard` / `typing` / `lid_open` / `carry` …）。
  存於 `fixtures/traces/`，用 Git LFS（若超過數 MB）。
- `trace-recorder` 開發工具：在真機錄製並以快捷鍵即時標註。
- CI：重播所有 trace 跑偵測器，計算 precision / recall，低於門檻（初訂 precision ≥ 0.95、recall ≥ 0.90）則失敗。
- 覆蓋率目標 ≥ 80%（`cargo llvm-cov`），前端用 Vitest。
- 每次發版前在真機跑手動檢查清單（`docs/release-checklist.md`）。

## Consequences

- 需要持續累積不同機型 trace；社群可回報 trace。
- 真機相關的 FFI 層（IOKit / CoreAudio）只能靠手動測試，盡量做薄。
