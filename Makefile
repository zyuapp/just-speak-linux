CARGO ?= cargo
MODEL_DIR ?= $(CURDIR)/models/parakeet-tdt-0.6b-v2-int8
GTK_BACKEND ?= wayland

.PHONY: build test check check-window check-panel model run install smoke pipewire-test benchmark

build:
	$(CARGO) build --release --locked

test:
	$(CARGO) test --locked

check:
	$(CARGO) fmt --check
	$(CARGO) clippy --locked --all-targets -- -D warnings
	$(CARGO) test --locked
	$(CARGO) build --locked
	python3 scripts/test-lifecycle.py
	gjs -m gtk/tests/lifecycle.js
	bash -n scripts/*.sh
	luac -p packaging/hyprland.lua

# Requires a display, GJS and GTK4; uses isolated state and fake desktop helpers.
check-window:
	$(CARGO) build --locked
	gjs -m gtk/tests/shortcut-recorder.js
	GDK_BACKEND=$(GTK_BACKEND) gjs -m gtk/tests/shortcut-recorder-window.js
	GDK_BACKEND=$(GTK_BACKEND) gjs -m gtk/main.js --smoke-test
	python3 scripts/test-window-workflow.py --backend $(GTK_BACKEND)

# Requires the installed Omarchy shell and Quickshell; no desktop actions.
check-panel:
	python3 scripts/test-inline-shortcut.py
	python3 scripts/test-inline-shortcut.py --lifecycle
	python3 scripts/test-inline-shortcut.py --workflows

model:
	./scripts/download-model.sh "$(MODEL_DIR)"

run: build
	target/release/just-speak --model-dir "$(MODEL_DIR)" daemon

install: build
	./scripts/install.sh

smoke: build
	python3 scripts/smoke-test.py --binary target/release/just-speak --model-dir "$(MODEL_DIR)"

pipewire-test:
	python3 scripts/test-pipewire.py

# FILE must point to a mono 16 kHz PCM16 WAV containing real speech.
benchmark: build
	@test -n "$(FILE)" || { echo 'Usage: make benchmark FILE=/path/to/speech.wav'; exit 2; }
	target/release/just-speak --model-dir "$(MODEL_DIR)" benchmark "$(FILE)"
