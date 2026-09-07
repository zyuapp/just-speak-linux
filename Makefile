CARGO ?= cargo
MODEL_DIR ?= $(CURDIR)/models/parakeet-tdt-0.6b-v2-int8

.PHONY: build test check model run install smoke pipewire-test benchmark

build:
	$(CARGO) build --release --locked

test:
	$(CARGO) test --locked

check:
	$(CARGO) fmt --check
	$(CARGO) clippy --locked --all-targets -- -D warnings
	$(CARGO) test --locked
	bash -n scripts/*.sh
	luac -p packaging/hyprland.lua

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
