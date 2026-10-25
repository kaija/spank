# Spank — common tasks. `make` (or `make help`) lists them.
#
#   make setup      first time: install JS deps and the pre-commit hook
#   make dev        app with hot reload (Next dev server + Tauri), settings window open
#   make app        release .app (unsigned) → target/release/bundle/macos/Spank.app
#   make release    signed .app + DMG (Developer ID, see docs/release.md)
#   make check      what CI runs: lint + tests

SHELL := /bin/bash
DESKTOP := apps/desktop
OUT := $(DESKTOP)/out/index.html
APP := target/release/bundle/macos/Spank.app
DIST := target/release-dist

# Signing identity for `make release` (override: make release SIGN_IDENTITY="…").
SIGN_IDENTITY ?= Developer ID Application: Chang KaiChieh (H2ZM466J6A)
# Updater key for the update archive + latest.json (leave empty to skip).
UPDATER_KEY ?= $(wildcard $(HOME)/.tauri/spank-updater.key)

.DEFAULT_GOAL := help
.PHONY: help setup frontend dev dev-app ui app dmg run release release-notarize \
        test test-rust test-ui lint fmt typecheck clippy check coverage bindings deny \
        traces sounds power clean

help: ## List the targets
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z_-]+:.*## / {printf "  \033[36m%-17s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# ---- setup -------------------------------------------------------------------------------------

setup: ## Install JS deps and the pre-commit hook (first time)
	pnpm install
	pre-commit install
	$(MAKE) frontend

# Tauri embeds apps/desktop/out at compile time: build it once before any Rust build of the app.
$(OUT):
	pnpm --dir $(DESKTOP) build

frontend: ## Build the static frontend (apps/desktop/out)
	pnpm --dir $(DESKTOP) build

# ---- local development -------------------------------------------------------------------------

dev: $(OUT) ## Run the app with hot reload and open settings
	pnpm --dir $(DESKTOP) tauri dev -- -- --open-settings

dev-app: $(OUT) ## Run the app with hot reload (menu bar only, as at login)
	pnpm --dir $(DESKTOP) tauri dev

ui: ## Settings UI in a browser at http://localhost:3000 (fake backend, no Tauri)
	pnpm --dir $(DESKTOP) dev

# ---- packages ----------------------------------------------------------------------------------

app: ## Release .app, unsigned (target/release/bundle/macos/Spank.app)
	pnpm --dir $(DESKTOP) tauri build --bundles app
	@echo "→ $(APP)"

dmg: ## Release .app + DMG, unsigned (Gatekeeper warns on other Macs)
	pnpm --dir $(DESKTOP) tauri build --bundles app,dmg
	@ls target/release/bundle/dmg/*.dmg

run: ## Open the built .app (build it with `make app` first)
	@test -d "$(APP)" || { echo "no $(APP): run 'make app' first"; exit 1; }
	open "$(APP)"

release: ## Signed .app + DMG with entitlements (needs the identity + signing/ profile)
	APPLE_SIGNING_IDENTITY="$(SIGN_IDENTITY)" TAURI_SIGNING_PRIVATE_KEY="$(UPDATER_KEY)" \
		tools/release.sh --out $(DIST)

release-notarize: ## Same as release, then notarize + staple (needs NOTARY_KEY_ID/ISSUER/PATH)
	APPLE_SIGNING_IDENTITY="$(SIGN_IDENTITY)" TAURI_SIGNING_PRIVATE_KEY="$(UPDATER_KEY)" \
		tools/release.sh --notarize --out $(DIST)

# ---- checks ------------------------------------------------------------------------------------

test: test-rust test-ui ## All tests (Rust + Vitest)

test-rust: $(OUT) ## Rust tests (includes the slap trace replay gate)
	cargo test --workspace

test-ui: ## Frontend tests (Vitest)
	pnpm --dir $(DESKTOP) test

fmt: ## Format Rust code
	cargo fmt --all

typecheck: ## TypeScript typecheck
	pnpm --dir $(DESKTOP) typecheck

clippy: $(OUT) ## Clippy with warnings as errors
	cargo clippy --workspace --all-targets -- -D warnings

lint: typecheck clippy ## fmt check + clippy + typecheck
	cargo fmt --all -- --check

check: lint test bindings ## What CI runs: lint, tests, bindings up to date

coverage: ## Line coverage of the library crates (CI gate: 80 %)
	cargo llvm-cov -p spank-core -p spank-store -p spank-audio -p spank-pack -p spank-sensors -p spank-sysmon \
		--summary-only --fail-under-lines 80

bindings: ## Regenerate TypeScript types from Rust (apps/desktop/src/bindings)
	cargo test -q -p spank-core --features ts export_bindings

deny: ## Dependency policy (cargo-deny)
	cargo deny check advisories bans licenses sources

# ---- data & hardware ---------------------------------------------------------------------------

traces: ## Slap detection precision / recall on the recorded traces
	cargo test -p spank-sensors --test slap_traces -- --nocapture

sounds: ## Rebuild the built-in sound packs (assets/sounds/builtin)
	cargo run --release -p gen-builtin-sounds

power: ## CPU use of the sensor pipeline (real MacBook)
	cargo run --release -p spank-sensors --example power-check

clean: ## Remove build output (target/, apps/desktop/out, .next)
	cargo clean
	rm -rf $(DESKTOP)/out $(DESKTOP)/.next
