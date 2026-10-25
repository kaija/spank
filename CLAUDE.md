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

`make` lists shortcuts for everything below (`make setup`, `make dev`, `make ui`, `make app`, `make dmg`, `make release`, `make check`, …); the Makefile builds the frontend first when `apps/desktop/out` is missing.

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
cargo llvm-cov -p spank-core -p spank-store -p spank-audio -p spank-pack -p spank-sensors -p spank-sysmon --summary-only   # coverage (target ≥ 80%)
cargo test -p spank-sensors --test slap_traces -- --nocapture  # slap precision/recall on the labeled traces (T23 gate)
cargo test -p spank-core --features ts export_bindings           # regenerate apps/desktop/src/bindings (CI fails if stale)
cargo deny check advisories bans licenses sources                # dependency policy (deny.toml)
```

**Frontend**

```bash
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop test                                     # Vitest (jsdom)
pnpm --dir apps/desktop exec vitest run src/i18n/locale.test.ts  # single test file
```

**App**

```bash
pnpm --dir apps/desktop tauri dev -- -- --open-settings   # dev: Next dev server on :3000 + Tauri (stop any other :3000 server first)
pnpm --dir apps/desktop dev                               # UI only, in a browser, against the in-memory mock backend
pnpm --dir apps/desktop tauri build --bundles app         # → target/release/bundle/macos/Spank.app
```

**Release** (T40, docs/release.md): `.github/workflows/release.yml` runs on `v*` tags. Locally:

```bash
APPLE_SIGNING_IDENTITY="Developer ID Application: Chang KaiChieh (H2ZM466J6A)" tools/release.sh   # add --notarize with NOTARY_KEY_ID/ISSUER/PATH
```

With `TAURI_SIGNING_PRIVATE_KEY=~/.tauri/spank-updater.key` it also writes the updater archive, `.sig` and `latest.json`. Before a release, walk `docs/release-checklist.md`. Release builds add `Spank.entitlements` and the embedded profile via `src-tauri/tauri.release.conf.json`; dev builds never carry them (the restricted entitlement would stop the app from launching without the profile).

**Mac App Store edition** (ADR-0017, `docs/app-store.md`): "Creaky", lid sounds only (the sandbox can't switch the accelerometer on), built with the `appstore` Cargo feature instead of the default `direct`; same bundle id `dev.spank.app`, profile `signing/Creaky_App_Store.provisionprofile`, no updater, no network. `tools/release-appstore.sh --local-test` runs a sandboxed build locally; `--upload` sends the `.pkg` to App Store Connect. Store copy, review notes and privacy answers are in `docs/app-store.md`; the privacy policy URL points at `docs/privacy.md`.

**Hardware probes** (real MacBook only; none need root)

```bash
cargo run --release -p spank-sensors --example sensor-check -- --secs 5 [--slaps]   # IoHidLid + IoHidAccel (+ live impacts)
cargo run --release -p spank-audio --example playback-latency -- --runs 10        # paused stream → play delay (< 50 ms)
cargo run -p spank-sysmon --example sysmon-watch -- --secs 60                     # prints every SysSignal change
cargo run -p lid-probe -- --mode poll --secs 10 --interval-ms 50
cargo run -p accel-probe -- --enable --secs 5                     # --ndjson to stream samples
cargo run --release -p audio-probe -- cpu paused --secs 20        # or: latency --runs 10 --gap-ms 15000
cargo run -p spank-sensors --example input-check -- --request     # Input Monitoring + key/click timing (T39)
cargo run --release -p spank-sensors --example power-check        # CPU of the sensor pipeline, slaps off/on (T42)
tools/focus-probe/bundle.sh --watch-secs 20                       # must run as a signed .app; writes target/focus-probe/output.txt
cargo run --release -p trace-recorder -- --check                  # both sensors OK? prints JSON
cargo run --release -p trace-recorder -- --scenario all --out fixtures/traces/<machine>-v1   # interactive, guides the user
python3 -I tools/analyze-trace.py fixtures/traces/m4air-v1        # per-segment peaks / naive detector results
tools/run-as-app.sh [--sign "Developer ID Application: …"] [--entitlements f] <pkg> -- <args>   # run a tool as a signed .app via LaunchServices
```

CI (`.github/workflows/ci.yml`) runs on `macos-latest`: frontend typecheck and build, then fmt, clippy, test and coverage. cargo-deny runs as a separate Ubuntu job. The remote is `github.com/kaija/spank`.

## Architecture (big picture)

**Single unprivileged process.** Spike S2 showed that both sensors can be read as a normal user, so there is no root helper and no IPC (ADR-0014). Keep the `ImpactCandidate` boundary clean anyway: if a future macOS release locks the sensors down, ADR-0003 (a privileged helper) is the fallback.

**Sensors** (`05AC:8104` SPU HID device; see design §5–6 and the spike report):
- **Lid angle** (page `0x20`/usage `0x8A`): **poll** the feature report (`GetReport(Feature, id=1)` → `[01, lo, hi]`, u16 degrees). The device's pushed input reports arrive at only ~1 Hz, so don't use them. Plan: poll at 5 Hz idle, 30 Hz while moving.
- **Accelerometer** (page `0xFF00`/usage `3`):
  - Start: set `SensorPropertyReportingState`, `SensorPropertyPowerState` and `ReportInterval` on `AppleSPUHIDDriver`. Reports then arrive at ~796 Hz: 22 bytes, x/y/z as i32 LE at offsets 6/10/14, ÷65536 = g.
  - Stop: set the properties back to 0, otherwise reporting keeps running.
  - **HID callbacks are `extern "C"`: a panic there aborts the process** (piping a probe into `head` crashed one this way). Callbacks should only push to a buffer; never `println!` or `unwrap` inside them.

**Crates:**
- `spank-core`: pure logic, `#![forbid(unsafe_code)]`. `ts` feature exports TS types via ts-rs.
- `spank-store`: on-disk layout of design §9. Atomic writes; corrupt files are renamed to `*.corrupt-<ms>` and replaced by defaults; `migrate.rs` holds config schema steps; ids double as file names, so `validate_id` guards them.
- `spank-audio`: import pipeline (symphonia 0.6 → rubato 5 `Fft` → trim → ebur128). Clips < 400 ms are looped for loudness measurement; > 2 channels are mixed to mono. Fixtures and their ffmpeg commands are in `tests/fixtures/`. Playback: `Mixer` (single voice, strictly-higher priority preempts with a 50 ms fade), `SoundBank` (preload, 30 MB cap), `AudioPlayer`.
- `spank-store::Profiles`: built-in profiles (`builtin.*`, read-only, loaded from a bundle dir) + user profiles + active profile with fallback.
- `spank-sensors`: `LidSource` / `AccelSource`, trace format v1 + `TraceReplay`, report parsers, `LidPollSchedule`, and on macOS `IoHidLid`, `IoHidAccel` (own CFRunLoop thread, SPSC ring buffer, properties reset to 0 on stop; call `reset_accel_properties()` at startup), `capabilities()`.
- `spank-sysmon`: `SysMonitor` emits deduplicated `SysSignal`s (CoreAudio/CoreMediaIO listeners, NSWorkspace + distributed notifications on the **main queue**, so the main run loop must run; Focus polled every 5 s). Mic detection uses CoreAudio process objects and skips our own pid, so our playback never counts as a meeting.
- `spank-pack`: `.spankpack` read/validate (untrusted input: paths, symlinks, sizes declared and actual, sha256, 48 kHz ≤ 10 s WAV), import (content-addressed `pack-<sha16>` sound ids), export. `fuzz/` is a standalone cargo-fuzz crate (nightly); `mutated_packs_never_panic` is the stable stand-in.

**Detection** (`spank-core`):
- Slaps (`impact::ImpactExtractor`, T23): per-axis 2 Hz high-pass → `|a| > max(0.15 g, 12 × noise floor)` opens an 80 ms window → accepted if its mean frequency ≥ 16 Hz and it isn't a rebound (< 0.45 × the previous peak within 250 ms) → 150 ms refractory.
  - Why: typing overlaps slaps in peak and frequency but raises the noise floor; lifting/carrying/lid motion stay ≤ 12 Hz; hard slaps echo ~30 % at +100–150 ms; fast combos are ~200 ms apart (a 300 ms refractory drops them).
  - Timing uses sample counts at the nominal 800 Hz, because reports arrive in ~20 ms batches sharing one timestamp.
  - Scored against hand-annotated onsets in `fixtures/traces/m4air-v1/expected.json` (precision/recall 1.00 on 23 slaps, one machine). Re-run `slap_traces` after touching any parameter.
  - Light/medium/hard thresholds come from per-user calibration, because measured light and medium slaps overlap.
- Lid: `lid::LidDetector` (opened on wake / closing below a threshold) with hysteresis and cooldown; it also reports `last_motion_ms` for the slap gate (ADR-0005). `LidOverbent` is continuous (ADR-0015): `opening()` is true while the lid rises inside `opening_start_deg..=overbend_threshold_deg` (30–120°); the orchestrator plays it, stops it with `Output::StopSound` when the lid rests or goes down, resumes via `PlayRequest::offset_ms`, and plays it once per opening.
- Slap gates, levels and combos: `slap::SlapClassifier`. Gate order: disabled → lid moving (+500 ms) → own playback (+200 ms) → precise typing (+300 ms) → below light level.
- Calibration (`calibration::Calibrator`, T36): its own extractor with a 0.06 g trigger; light → medium → hard must each be ≥ 1.2× the previous slap; typing 5 s → p95 noise floor. Levels are geometric means between the measured slaps; the light level stays above typing. While calibrating the orchestrator keeps the accelerometer on (even when paused or slaps are off), plays nothing and emits `Output::Calibration` / `Output::AccelPeak`.
- Precise typing (T39): `spank-sensors::InputMonitor` (IOHIDManager) records only *when* a key or button changed. Needs Input Monitoring (`input_access()`); without it nothing starts and the T23 noise-floor filter is the only typing defense.
- Table bumps (1.3–1.9 g) deliberately count as slaps (ADR-0006).
- Playback priority order is in `EventKind::priority` (ADR-0008).
- Auto-mute: `mute::AutoMute` / `evaluate` (meeting, focus, quiet hours across midnight, system mute or volume 0). Wall time is passed in as `TimeOfDay`.
- `Orchestrator` (design §4.3) is pure logic: feed it lid samples, accel batches, `SysSignal`s and config; it returns `Output`s (sensor start/stop, `Fired`, `MuteChanged`) and plays through the `Player` trait. Sensors run only while awake, unlocked and not paused; after a wake the lid is read once even when locked, so `LidOpened` isn't delayed until unlock. `crates/spank-sensors/tests/orchestrator_replay.rs` replays traces through it.
- Defaults and validation live in `crates/spank-core/src/config.rs`.

**Audio:** create the cpal output stream once at startup, keep it **paused**, `play()` it on trigger (~20–40 ms), then pause again. An always-running silent stream costs coreaudiod ~12% CPU, and the first open in a process can take ~1.9 s (spike S5). `spank-audio::AudioPlayer` does this (caller → mpsc → control thread owning the `cpal::Stream` → `rtrb` → callback owning the `Mixer`); the mixer resamples and maps channels on the fly, so clips stay at 48 kHz.

**App shell** (`apps/desktop/src-tauri`):
- Menu bar only: `LSUIElement` in `Info.plist` plus `ActivationPolicy::Accessory`.
- `window::open_settings` creates the `settings` WebView on demand and switches to `Regular` (Dock icon shows).
- The WebView is destroyed on close; the `Destroyed` event switches back to `Accessory`.
- `RunEvent::ExitRequested { code: None }` is prevented, so only the tray's Quit exits.
- A second launch (single-instance plugin) or the `--open-settings` arg opens settings.
- The autostart plugin is registered, but launch-at-login must only be enabled from config (T14/T38), never implicitly in dev.

**Frontend** (`apps/desktop`):
- Next.js with `output: "export"` and `trailingSlash: true` only (no SSR, API routes or image optimization). Each route is `<route>/index.html`.
- Settings pages live in the `src/app/(settings)/` route group wrapped by `SettingsShell` (sidebar + header from the mockup). `/` redirects client-side to `/profiles/`.
- i18n is client-side `next-intl` (`src/i18n`): no i18n routing. Pages prerender in zh-Hant, then switch to `resolveLocale(setting, navigator.languages)`. Keep `messages/zh-Hant.json` and `en.json` key-identical (a test checks this).
- Types shared with Rust come from `src/bindings/` (generated by ts-rs, do not edit). We use ts-rs because tauri-specta 2 is still a release candidate.
- Styling: `src/styles/tokens.css` is copied from the mockup design system, plus the dark palette. Inter is bundled via `@fontsource-variable/inter` because remote fonts are blocked offline and by the CSP. Icons are in `src/ui/iconPaths.ts`, copied from `mockup/spank-icons.js`.
- The window uses an overlay title bar (native traffic lights over the sidebar); elements with `data-tauri-drag-region` move the window.
- It talks to Rust only through `src/backend/` (`Backend` interface): `tauriBackend` inside the app, `createMockBackend()` in a plain browser (`pnpm dev` at localhost:3000 works without Tauri; in the browser it runs in demo mode, faking the waveform and calibration slaps) and in tests (`src/test/render.tsx`). `BackendProvider` sits in the root layout, so `AppI18nProvider` can follow `AppConfig.language`; `useProfiles` / `useSounds` / `useConfig` / `useCalibration` / `useRuntimeState` / `useAccelWave` reload on backend events.
- The onboarding wizard is `src/screens/OnboardingWizard.tsx`, rendered by `app/onboarding/` and `app/calibrate/` (outside the settings route group).
- Page components live in `src/screens/` (not `src/pages/`, which Next would treat as Pages Router routes); `src/app/(settings)/<route>/page.tsx` just renders them. Shared controls (Button, Switch, Slider, Badge, Tag, Sheet) are in `src/ui/controls.tsx`; a page puts buttons in the header with `<HeaderActions>`.
- Built-in sound and profile names are translated in the message files (`sounds.*`, `builtinProfiles.*`); use `soundName` / `profileName` from `src/lib/names.ts`.

**App runtime** (`apps/desktop/src-tauri/src`):
- `engine.rs`: the core thread. Owns the `Orchestrator`, `AudioPlayer`, `IoHidLid` (polled per `LidPollSchedule`) and `IoHidAccel` (drained every 50 ms), starts/stops them on the orchestrator's `SensorCommand`s, and reports `EngineEvent`s. Sensors and player share one `Instant` epoch.
- `state.rs`: `AppState` (store, built-in set, `Profiles`, `config` mutex, engine). A command changes the config, saves it, then sends it to the engine (`update_config`).
- `commands.rs`: the UI's backend (design §10.3); emits `profiles_changed` / `library_changed`. `tray.rs` rebuilds the menu when paused / mute / active profile / profile list change.
- Built-in profiles and sounds come from `assets/sounds/builtin/*.spankpack` (bundled as resource `builtin/`), loaded with `spank_pack::load_builtin`. Two built-in profiles (ADR-0016): `builtin.destruction` = 全玻璃 (All Glass) and `builtin.classic` = 地城石門 (Dungeon Gate), each one material across all seven events. `assets/sounds/elevenlabs.json` is the single source: profile names, each sound's prompt, event and length, plus `kept_sounds` (earlier hand-picked recordings) and `parked_profiles` (designed, not built in).
  - New candidates: `python3 -I tools/elevenlabs-sounds.py generate --key ~/Desktop/code/spank/signing/elevenlabs.key [--only STEM…]`, then `page` (listening page in `target/sound-candidates/`), then `import picks.json` (copies picks to `assets/sounds/source/<stem>.wav`). The key file is gitignored, scoped to sound generation, and never printed.
  - Pack them with `cargo run --release -p gen-builtin-sounds` (deterministic): recordings are mixed to mono and peak-limited (≤ 12 dB) so they get close to -16 LUFS. All built-in sounds are generated by the author with ElevenLabs and licensed only for use in Spank — see `assets/sounds/SOUNDS_LICENSE.md`. Budget: < 8 MB uncompressed.
  - Don't prompt for imitations of specific works' signature sounds; describe the genre instead.
- `SysMonitor` must be started from setup (main thread); a panic hook resets the accelerometer properties.
- The spike tools in `tools/` still use their own copies in `tools/common/` rather than `spank-sensors`.
- Windows (`window.rs`): `settings` and `onboarding` (route `onboarding/` on first launch when `onboarding_done` is false, or `calibrate/` from Sensors › Recalibrate). Closing the onboarding window cancels calibration and counts as onboarding done. The Dock icon shows while any window is open.
- `AppState::update_config` saves, sends the config to the engine, emits `config_changed` and refreshes the tray. The UI's `useConfig` reloads on that event (not on `engine_state`, which fires with every lid angle change).
- Launch at login: `commands::apply_launch_at_login` (autostart plugin). Startup syncs it with the config **only in release builds**.
- Logs: launched outside a terminal, stderr is redirected to `~/Library/Logs/dev.spank.app/spank.log` (`diag.rs`).
- Updates (`update.rs`, T41): `tauri-plugin-updater` reads `latest.json` from GitHub Releases (`plugins.updater` in `tauri.conf.json`, public key there). Release builds check 2 min after launch, then daily, and ask with a native dialog; dev builds never check on their own. The private key is `~/.tauri/spank-updater.key` (outside the repo; losing it means installed copies can't be updated).

**Pinned versions:**
- Tauri **2.x** (3.x is still alpha).
- TypeScript **6.x**: TS 7 is the native port, and Next.js needs the JS compiler API.
- pnpm 11 refuses unapproved install scripts. `allowBuilds` in `pnpm-workspace.yaml` denies the two we don't need (`@swc/core`, `@parcel/watcher`).

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
- **Signing:** a Developer ID Application identity (team `H2ZM466J6A`) is in the keychain. Both ad-hoc and Developer ID–signed hardened-runtime `.app`s read both sensors (T09).
- **Focus status:** `INFocusStatusCenter.isFocused` only works when the app is signed with Developer ID and carries the Communication Notifications entitlement plus an embedded provisioning profile.
  - The App ID is `H2ZM466J6A.dev.spank.app`. The profile lives in gitignored `signing/Spank_Developer_ID.provisionprofile` and must never be committed.
  - Test it with `tools/run-as-app.sh --sign "Developer ID Application: Chang KaiChieh (H2ZM466J6A)" --bundle-id dev.spank.app --profile signing/Spank_Developer_ID.provisionprofile --entitlements tools/focus-probe/communication-profiled.entitlements focus-probe`.
  - Without the profile, the restricted entitlement makes AMFI refuse to launch ("No matching profile found"), and without the entitlement `isFocused` is always false.
  - Renaming the bundle ID means registering a new App ID and profile.
