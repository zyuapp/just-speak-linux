# JustSpeak for Linux

Local push-to-talk dictation for **Omarchy 4 / Hyprland 0.56**. Hold F9, speak,
release to transcribe and paste. Escape cancels. The model stays in memory in a
small Rust service; Quickshell provides an optional bar control and recording
indicator that never takes keyboard focus.

This is an initial Linux implementation of [JustSpeak](https://github.com/zyuapp/just-speak).
It uses English Parakeet TDT 0.6B v2 through sherpa-onnx on the CPU. NVIDIA/CUDA
inference is not implemented in this build. See [measured verification](docs/verification.md)
for performance and the limits of testing on the development machine.

## Build and try

Requirements: Rust 1.88+, a C/C++ toolchain, PipeWire audio tools (`pw-record`),
`wl-clipboard`, and Hyprland with Lua configuration. The model downloader also
uses Bash, curl, Python 3, coreutils, and util-linux. Quickshell 0.3 is optional.
Lua is needed to run the compositor-integration unit tests.

```sh
make build
make model
make run
```

Keep that daemon running. From another terminal in the same desktop session:

```sh
target/release/just-speak status
target/release/just-speak start
# Speak, then:
target/release/just-speak stop
# Or: target/release/just-speak cancel
```

The one-time model download is about 483 MB (decimal), with roughly 661 MB of
installed files. Its official release archive is pinned by SHA256 and extracted
into a staging directory before publication. Transcription runs entirely locally.
The first Rust build downloads Cargo dependencies and sherpa-onnx's static native
runtime; subsequent builds can use their caches. The model is separate from Git
and from the executable.

For file transcription without touching the clipboard:

```sh
target/release/just-speak --model-dir models/parakeet-tdt-0.6b-v2-int8 \
  transcribe models/parakeet-tdt-0.6b-v2-int8/test_wavs/0.wav
```

## Install and integrate

```sh
make install
# Ensure ~/.local/bin is on PATH in your desktop session.
just-speak model download
systemctl --user daemon-reload
systemctl --user enable --now just-speak.service
just-speak doctor
```

The installed service uses the default XDG model directory. To reuse the model
downloaded by `make model`, set its **absolute** path as `model_dir` in the config
below instead of downloading again.

See [desktop integration](packaging/README.md) for the F9/Escape binding snippet,
Omarchy bar widget, optional standalone overlay, Arch package, and removal steps.
**F9 already belongs to Voxtype in Omarchy when Voxtype is installed**; the
provided binding snippet explicitly replaces it. The installer only copies app
files and user service units. It does not change your hotkeys, start services,
or enable a bar widget.

## Configuration

Optional `${XDG_CONFIG_HOME:-~/.config}/just-speak/config.toml`:

```toml
num_threads = 6 # Default: available CPU parallelism, capped at 6.
paste = true
max_recording_seconds = 120
# model_dir = "/absolute/path/to/parakeet-tdt-0.6b-v2-int8"
# input = "PipeWire node name or object serial"
```

Restart the service after changing settings. `--model-dir` and `--threads`
override settings for the process being launched; they do not reconfigure an
already-running daemon. `just-speak model path` prints the configured model path.
With `paste = false`, completed dictation is copied to the clipboard only.

## Behavior and limits

- Audio is captured only while recording, as private temporary 16 kHz mono PCM16
  WAV. Completed/canceled recordings are deleted during normal operation and
  graceful shutdown. A forced kill or machine crash can leave private temporary
  files until system temporary-file cleanup.
- Hold/release events control recording directly. There is no silence timeout;
  the configurable recording limit is 1–120 seconds. File input has a 121-second
  safety bound to accommodate capture shutdown timing.
- Cancellation immediately invalidates pending output. Native inference already
  in progress finishes internally, and its result is discarded. Clipboard
  delivery checks cancellation again before requesting a paste; a paste already
  dispatched cannot be retracted.
- Automatic paste requires the original window to remain focused, with the same
  address, class, and process. If focus changed, text stays on the clipboard and
  status explains why paste was skipped. It never sends Enter. Terminals use
  Ctrl+Shift+V; other applications use Ctrl+V. Custom app shortcuts may require
  manual paste. The previous clipboard content is replaced.
- Escape is nonconsuming: it cancels dictation and also reaches the focused app.
  Starting while transcribing is rejected unless the pending result is canceled.
  Repeated start/stop events are idempotent.
- Exact digital silence and taps shorter than 100 ms are discarded. Very short
  speech is padded for inference. Background noise is not classified by a VAD.
- English dictation only, CPU only, and this Hyprland version family only. There
  is no server, LLM rewriting, transcript history, or telemetry.

## Verification

```sh
make check
make smoke                    # Uses fake capture/paste, with the real local model.
make pipewire-test            # Isolated native PipeWire server; no real microphone.
make benchmark FILE=/path/to/real-speech.wav
JUST_SPEAK_TEST_MODEL_DIR="$PWD/models/parakeet-tdt-0.6b-v2-int8" \
  cargo test real_model -- --ignored
```

The benchmark excludes model loading from resident inference, reports first-use
latency separately, and returns failure below **20× real time**. Use `--threads`
to compare CPU settings; `--expected-text` checks a phrase in every transcript.
Short-utterance response and full microphone-to-paste latency need separate
measurement. The smoke test uses isolated XDG directories and fake desktop/audio
helpers, so it does not record your microphone or type into your applications.

Runtime state is available through `status --json` and `watch` (newline JSON).
The daemon uses a private Unix socket under `$XDG_RUNTIME_DIR/just-speak` with a
single-instance lock. Status and service logs omit transcript text.

Original source: MIT. The current local executable also links GPL-licensed
eSpeak code from the upstream native runtime. Runtime and model terms, including
this distinction for binary distribution, are documented in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
