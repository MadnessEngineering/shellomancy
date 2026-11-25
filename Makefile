# Shellomancy's dev loop. `make` lists the targets.
#
# /Applications/Shellomancy.app is the daily driver. `make dev` runs a second copy built in
# target/, beside it, so trying a change never means quitting the live app and the Claude
# sessions in its tabs. `make install` is for when a build should become the daily driver.

.DEFAULT_GOAL := help
SHELL := /bin/bash

# Names the dev copy's data: ~/.mad/shellomancy-$(PROFILE) and friends.
PROFILE ?= dev
# Test-name filter for `make test`.
T ?=
# Arguments for `make dev-client`.
ARGS ?=

DEV_BUNDLE_DIR := target/debug/bundle
DEV_BINARY := bundle/osx/Shellomancy.app/Contents/MacOS/warp-oss
INSTALLED_APP := /Applications/Shellomancy.app
DEV_DISCOVERY_DIR := $(HOME)/.mad/local-control-$(PROFILE)
DEV_SETTINGS_DIR := $(HOME)/.mad/shellomancy-$(PROFILE)
DEV_DOMAIN := cc.madnessinteractive.shellomancy-$(PROFILE)
DEV_STATE_DIR := $(HOME)/Library/Application Support/$(DEV_DOMAIN)
DEV_APP_LOG := $(HOME)/Library/Logs/shellomancy-$(PROFILE).log
DEV_RUN_LOG := target/make-dev-$(PROFILE).log
LSREGISTER := /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
CLIENT := python3 script/shellomancy_client/shellomancy_client.py

# PIDs of this profile's running dev copies: live discovery records whose process is still the
# dev binary (records outlive a killed app, and PIDs get reused).
DEV_PIDS = $$(for pid in $$(grep -ho '"pid": *[0-9]*' "$(DEV_DISCOVERY_DIR)"/*.json 2>/dev/null \
	| grep -o '[0-9]*$$'); do ps -o command= -p $$pid 2>/dev/null | grep -q '$(DEV_BINARY)' \
	&& echo $$pid; done)

.PHONY: help profile-check dev dev-bg dev-stop dev-log dev-reset dev-client dev-glow-demo \
	instances install install-open check test test-client fmt

help: ## List these targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*## "} {printf "  make %-13s %s\n", $$1, $$2}'

profile-check:
	@case "$(PROFILE)" in ""|*/*|*" "*) echo "PROFILE must be a plain name (got '$(PROFILE)')" >&2; \
		exit 2;; esac

# WARP_DATA_PROFILE (honored by debug builds only) gives the copy its own settings dir, session
# database, UserDefaults and Keychain items. Without it the copy would restore your windows and
# its saves would overwrite the live app's session. Its own local-control discovery dir keeps
# hooks, which pick the newest instance, talking to the live app; the profile also gets its own
# log (~/Library/Logs/shellomancy-$(PROFILE).log when stdout isn't a terminal). `--format osx`
# skips cargo bundle's default ~150 MB dmg. The bundle in target/ has the installed app's
# bundle id, so on exit it's unregistered and removed (with any dmg left by older runs) and
# the installed app re-registered; otherwise Launch Services may open the target/ copy later.
# CLAUDE* variables are dropped first: started from inside Claude Code, the copy would otherwise
# hand Claude's session (CLAUDECODE, CLAUDE_CODE_VERSION, ...) to every tab, so a claude run
# there would think it's nested and its plugin would answer through stdout instead of the tab.
dev: profile-check ## Build and run an isolated copy beside the installed app; returns when it quits
	@bash -c 'forget() { for app in $(DEV_BUNDLE_DIR)/*/Shellomancy.app; do \
		[ -d "$$app" ] || continue; "$(LSREGISTER)" -u "$$app" 2>/dev/null; rm -rf "$$app"; done; \
		rm -f $(DEV_BUNDLE_DIR)/dmg/Shellomancy.dmg; \
		if [ -d "$(INSTALLED_APP)" ]; then "$(LSREGISTER)" -f "$(INSTALLED_APP)"; fi; }; \
		trap forget EXIT; \
		for var in $$(compgen -e | grep "^CLAUDE"); do unset "$$var"; done; \
		WARP_DATA_PROFILE="$(PROFILE)" WARP_LOCAL_CONTROL_DISCOVERY_DIR="$(DEV_DISCOVERY_DIR)" \
		./script/run --format osx; status=$$?; \
		if [ $$status = 143 ]; then status=0; fi; exit $$status'

dev-bg: profile-check ## make dev in the background, so the prompt comes back; make dev-stop ends it
	@if [ -n "$(DEV_PIDS)" ]; then echo "A dev copy is already running (make dev-stop)." >&2; \
		exit 1; fi
	@echo "=== make dev-bg, $$(date)" >> "$(DEV_RUN_LOG)"
	@nohup $(MAKE) --no-print-directory dev PROFILE=$(PROFILE) >> "$(DEV_RUN_LOG)" 2>&1 &
	@echo "Building and starting the dev copy in the background."
	@echo "  build output: $(DEV_RUN_LOG)"
	@echo "  app log:      $(DEV_APP_LOG)  (make dev-log)"

dev-stop: profile-check ## Quit the dev copy and wait for it; make dev's cleanup then runs
	@pids="$(DEV_PIDS)"; if [ -z "$$pids" ]; then echo "No dev copy running (PROFILE=$(PROFILE))."; \
		exit 0; fi; kill -TERM $$pids; \
		for _ in $$(seq 60); do kill -0 $$pids 2>/dev/null || break; sleep 1; done; \
		if kill -0 $$pids 2>/dev/null; then echo "The dev copy (pid $$pids) is still quitting." >&2; \
		exit 1; fi; echo "Stopped the dev copy (pid $$pids)."

dev-log: profile-check ## Follow the dev copy's app log
	tail -F "$(DEV_APP_LOG)"

dev-reset: profile-check ## Wipe the dev copy's settings, session and windows for a fresh start
	@if [ -n "$(DEV_PIDS)" ]; then echo "Quit the dev copy first (make dev-stop)." >&2; exit 1; fi
	rm -rf "$(DEV_SETTINGS_DIR)" "$(DEV_STATE_DIR)" "$(DEV_DISCOVERY_DIR)" \
		"target/glow-demo-$(PROFILE).tabs"
	@defaults delete "$(DEV_DOMAIN)" 2>/dev/null || true
	@echo "Reset PROFILE=$(PROFILE); the next make dev starts fresh."

dev-client: ## Control client against the dev copy: make dev-client ARGS="tab-move-to-window --quake"
	@WARP_LOCAL_CONTROL_DISCOVERY_DIR="$(DEV_DISCOVERY_DIR)" $(CLIENT) $(ARGS)

# Each tab runs script/glow_demo/claude, which feeds the bundled Claude plugin's hooks made-up
# input; the tabs open in the background, so a finished turn counts as unseen and glows. A rerun
# closes the tabs the last run opened. The glow shows in vertical tabs.
dev-glow-demo: profile-check ## In the dev copy, open a pretend Claude tab per glow: working, needs you, done, failed
	@WARP_LOCAL_CONTROL_DISCOVERY_DIR="$(DEV_DISCOVERY_DIR)" python3 script/glow_demo/demo_tabs.py \
		"target/glow-demo-$(PROFILE).tabs"

instances: ## Running Shellomancy copies: the installed app's and this profile's dev copy
	@echo "installed app:"; $(CLIENT) instances
	@echo "dev copy (PROFILE=$(PROFILE)):"; WARP_LOCAL_CONTROL_DISCOVERY_DIR="$(DEV_DISCOVERY_DIR)" \
		$(CLIENT) instances

install: ## Build and install /Applications/Shellomancy.app (quit Shellomancy first)
	./script/install_local

install-open: ## make install, then launch it
	./script/install_local --open

check: ## cargo check the app and its tests
	cargo check -p warp --tests

test: ## App unit tests whose names match T: make test T=move_to_window
	cargo test -p warp --lib -- $(T)

test-client: ## Python control-client tests
	python3 -m unittest discover -s script/shellomancy_client

fmt: ## Format all Rust code (script/format)
	./script/format
