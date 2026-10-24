# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`spank` (codename; final product name and bundle ID are undecided, ADR-0012) is a macOS menu bar app for Apple Silicon MacBooks on macOS 14+. It plays sounds when the lid is opened, closed low, or bent too far back, and "cries" when the laptop is slapped. Sounds are graded by slap strength, and quick repeated slaps trigger a combo. It is written in Rust with a Tauri v2 shell, open source under MIT, and published from a personal GitHub account.

## Source of truth: `docs/`

The docs are written in Traditional Chinese. Code, comments and commit messages are in English.

- `docs/adr/`: architecture decisions. Read the relevant ADR before changing behavior it covers. **ADR-0003 (root helper) is superseded by ADR-0014.**
- `docs/design.md`: architecture, data flow, config layout, UI pages, and Tauri commands.
- `docs/tasks.md`: Phase 1 work as tasks `T01`–`T44` grouped into **waves**. Tasks in the same wave have no dependencies on each other.
  - After finishing a task, add `- 狀態：✅ 完成（date…）` under its `- 依賴：` line.
  - Then regenerate the overview with `python3 -I tools/gen-task-overview.py`. It also fails if a task depends on a task in the same or a later wave.
- `docs/spike-report.md`: measured hardware facts. Parameters in the code come from here; don't change them without new measurements.
- `docs/ui-design-prompt.md`: the prompt that produced `mockup/`.

## Commands

**Setup**

```bash
pnpm install && pre-commit install
```

The pre-commit hook runs `cargo fmt --check` and `cargo clippy -D warnings`.

**Build the frontend first.** Tauri embeds `apps/desktop/out` at compile time, so in a fresh checkout `cargo build`/`clippy` of `spank-desktop` fails until the frontend is built:

```bash
pnpm --dir apps/desktop build
```

**Rust**

```bash
cargo test --workspace
cargo test -p spank-core profile::tests::duplicate_is_writable   # single test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo llvm-cov -p spank-core --summary-only                      # coverage (target ≥ 80%)
cargo deny check advisories bans licenses sources                # dependency policy (deny.toml)
```

**Frontend**

```bash
pnpm --dir apps/desktop typecheck
```

There are no UI tests yet; Vitest arrives in T15.

**App**

```bash
pnpm --dir apps/desktop tauri dev -- -- --open-settings   # dev: Next dev server on :3000 + Tauri
pnpm --dir apps/desktop tauri build --bundles app         # → target/release/bundle/macos/Spank.app
```

**Hardware probes** (real MacBook only; none need root)

```bash
cargo run -p lid-probe -- --mode poll --secs 10 --interval-ms 50
cargo run -p accel-probe -- --enable --secs 5                     # --ndjson to stream samples
cargo run --release -p audio-probe -- cpu paused --secs 20        # or: latency --runs 10 --gap-ms 15000
tools/focus-probe/bundle.sh --watch-secs 20                       # must run as a signed .app; writes target/focus-probe/output.txt
tools/record-spike.sh fixtures/traces/<name>                      # interactive: prompts the user to slap / type / move the lid
python3 -I tools/analyze-spike.py fixtures/traces/<name>
```

CI (`.github/workflows/ci.yml`) runs on `macos-latest`: frontend typecheck and build, then fmt, clippy, test and coverage. cargo-deny runs as a separate Ubuntu job. The repo has no remote yet, so CI has never run on GitHub.

## Architecture (big picture)

**Single unprivileged process.** Spike S2 showed that both sensors can be read as a normal user, so there is no root helper and no IPC (ADR-0014). Keep the `ImpactCandidate` boundary clean anyway: if a future macOS release locks the sensors down, ADR-0003 (a privileged helper) is the fallback.

**Sensors** (`05AC:8104` SPU HID device; see design §5–6 and the spike report):
- **Lid angle** (page `0x20`/usage `0x8A`): **poll** the feature report (`GetReport(Feature, id=1)` → `[01, lo, hi]`, u16 degrees). The device's pushed input reports arrive at only ~1 Hz, so don't use them. Plan: poll at 5 Hz idle, 30 Hz while moving.
- **Accelerometer** (page `0xFF00`/usage `3`):
  - Start: set `SensorPropertyReportingState`, `SensorPropertyPowerState` and `ReportInterval` on `AppleSPUHIDDriver`. Reports then arrive at ~796 Hz: 22 bytes, x/y/z as i32 LE at offsets 6/10/14, ÷65536 = g.
  - Stop: set the properties back to 0, otherwise reporting keeps running.
  - **HID callbacks are `extern "C"`: a panic there aborts the process** (piping a probe into `head` crashed one this way). Callbacks should only push to a buffer; never `println!` or `unwrap` inside them.

**Detection** (`spank-core`, pure logic, `#![forbid(unsafe_code)]`):
- Slaps: per-axis 2 Hz high-pass → |a| > 0.2 g → 80 ms window → 300 ms refractory.
  - Measured: typing ≤ 0.09 g, slaps ≥ 0.34 g, combo slaps ~330 ms apart. A refractory > 300 ms would drop combos.
  - Light/medium/hard thresholds come from per-user calibration, because measured light and medium slaps were nearly identical.
- Lid: event state machine (opened on wake / closing below a threshold / overbent) with hysteresis and cooldown (ADR-0005).
- Playback priority order is in `EventKind::priority` (ADR-0008).
- Defaults and validation live in `crates/spank-core/src/config.rs`.

**Audio:** create the cpal output stream once at startup, keep it **paused**, `play()` it on trigger (~20–40 ms), then pause again. An always-running silent stream costs coreaudiod ~12% CPU, and the first open in a process can take ~1.9 s (spike S5).

**App shell** (`apps/desktop/src-tauri`):
- Menu bar only: `LSUIElement` in `Info.plist` plus `ActivationPolicy::Accessory`.
- `window::open_settings` creates the `settings` WebView on demand and switches to `Regular` (Dock icon shows).
- The WebView is destroyed on close; the `Destroyed` event switches back to `Accessory`.
- `RunEvent::ExitRequested { code: None }` is prevented, so only the tray's Quit exits.
- A second launch (single-instance plugin) or the `--open-settings` arg opens settings.
- The autostart plugin is registered, but launch-at-login must only be enabled from config (T14/T38), never implicitly in dev.

**Frontend** (`apps/desktop`): Next.js with `output: "export"` only (no SSR, API routes or image optimization). It talks to Rust through `invoke` (for example `app_info`, which returns `AppConfig::default()`). The planned command list is in design §10.3.

**Planned crates** (design §3): `spank-sensors`, `spank-audio`, `spank-sysmon`, `spank-pack`. They don't exist yet; the hardware code currently lives in `tools/*` probes (shared FFI helpers in `tools/common/iokit.rs`).

**Pinned versions:**
- Tauri **2.x** (3.x is still alpha).
- TypeScript **6.x**: TS 7 is the native port, and Next.js needs the JS compiler API.

## UI mockups: `mockup/`

These are high-fidelity designs from Claude Design, generated from `docs/ui-design-prompt.md`. They are the visual reference for all UI work (T15, T31–T38). Match layout, states and copy to them.

| File | Contents |
|------|----------|
| `Spank UI.dc.html` | Overview canvas: every screen, numbered 1–6 (menu bar, settings pages 2a–2e, onboarding, dialogs, component states, interaction notes) |
| `Settings Window.dc.html` | Profiles, sound library, sensors, auto-mute, general |
| `Menu Bar.dc.html` | Tray icon states and dropdown |
| `Onboarding.dc.html` | Welcome → lid demo → pick profile → enable slaps → calibration → done |
| `Dialogs.dc.html` | Dialogs |
| `Component States.dc.html` | Per-component state variants |

- **How the files work:**
  - Each file is a `<x-dc>` canvas document with `{{ … }}` template bindings, rendered by `mockup/support.js`.
  - `support.js` is generated and must not be edited. It loads React and Babel from unpkg, so viewing needs a network connection.
  - Open a file directly in a browser to view it.
  - `{{ appName }}` is the product-name placeholder (ADR-0012).
- **Styling:**
  - The mockups use the **Arlo AI Design System** vendored in `mockup/_ds/arlo-ai-design-system-*/`. Its `readme.md` documents color, type, radii and icon rules. Tokens are in `tokens/*.css`, app-UI classes in `css/ui.css`, and component props contracts in `.d.ts` files.
  - App-specific icons are SVG path strings in `mockup/spank-icons.js` (`window.SpankIcons`: app states, event glyphs, …).
- **When implementing**, rebuild the screens as React components in `apps/desktop/src`. Don't load `support.js` or `_ds_bundle.js` into the app.

## Working notes

- **Git:** use the personal identity (`git-user personal`; the script switches the global git and SSH config). Commit only when asked.
- **Verifying UI:** don't take full-screen screenshots with `screencapture`; it captures the user's private windows. Ask the user to confirm what's visible instead.
- **Detecting the Dock icon:** `lsappinfo`'s `ApplicationType` does not update after switching back to `Accessory`, so it can't verify Dock icon state.
- **Open spike items** (T09):
  - Re-verify sensor access inside a Developer ID–signed, hardened-runtime `.app`.
  - Retest Focus status: `INFocusStatusCenter` authorizes, but `isFocused` stayed false under ad-hoc signing. It likely needs the Communication Notifications entitlement.
