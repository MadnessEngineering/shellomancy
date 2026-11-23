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

DEV_APP := target/debug/bundle/osx/Shellomancy.app
INSTALLED_APP := /Applications/Shellomancy.app
DEV_DISCOVERY_DIR := $(HOME)/.mad/local-control-$(PROFILE)
LSREGISTER := /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
CLIENT := python3 script/shellomancy_client/shellomancy_client.py

.PHONY: help dev dev-client install install-open check test test-client fmt

help: ## List these targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*## "} {printf "  make %-13s %s\n", $$1, $$2}'

# WARP_DATA_PROFILE (honored by debug builds only) gives the copy its own settings dir, session
# database, UserDefaults and Keychain items. Without it the copy would restore your windows and
# its saves would overwrite the live app's session. Its own local-control discovery dir keeps
# hooks, which pick the newest instance, talking to the live app. The copy shares the installed
# app's bundle id, so on exit its bundle is unregistered and removed and the installed app
# re-registered; otherwise Launch Services may open the target/ copy later.
dev: ## Build and run an isolated copy beside the installed app (PROFILE=dev)
	@[ -n "$(PROFILE)" ] || { echo "make dev: PROFILE must not be empty" >&2; exit 2; }
	@bash -c 'forget() { "$(LSREGISTER)" -u "$(DEV_APP)" 2>/dev/null; rm -rf "$(DEV_APP)"; \
		if [ -d "$(INSTALLED_APP)" ]; then "$(LSREGISTER)" -f "$(INSTALLED_APP)"; fi; }; \
		trap forget EXIT; \
		WARP_DATA_PROFILE="$(PROFILE)" WARP_LOCAL_CONTROL_DISCOVERY_DIR="$(DEV_DISCOVERY_DIR)" \
		./script/run'

dev-client: ## Control client against the dev copy: make dev-client ARGS="tab-move-to-window --quake"
	@WARP_LOCAL_CONTROL_DISCOVERY_DIR="$(DEV_DISCOVERY_DIR)" $(CLIENT) $(ARGS)

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
