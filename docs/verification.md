# Verification on the development machine

Date: 2026-09-06. This is a functional prototype, **not a claim that the original
20× speed requirement has been met**.

## Machine and build

- AMD Ryzen 5 5600H, six physical cores / twelve logical CPUs, about 30 GiB RAM
  reported by Linux.
- NVIDIA GeForce GTX 1650, 4 GiB VRAM, driver 610.57.04. GPU diagnostics work
  outside the execution sandbox; an initial sandboxed diagnostic incorrectly
  appeared to show an unavailable driver.
- Omarchy 4.0.2, Hyprland 0.56.2, Quickshell 0.3.1, PipeWire 1.6.8.
- Rust 1.98.1, release optimization, sherpa-onnx 1.13.7 with its default static
  CPU runtime. The executable is approximately 36 MB and dynamically requires
  only the standard C/C++ system libraries (`ldd` checked).

## CPU inference

The official downloaded model contains `test_wavs/0.wav`, a 7.435-second real
speech sample. Concatenating it eight times produces a 59.48-second test file.
This is repeated real speech, **not a naturally spoken one-minute recording**;
the timings do not establish accuracy for varied speech, accents, noise, or
technical vocabulary. Each measured run required a nonempty transcript
containing “old portrait”.

| CPU threads | Slowest resident inference | Speed |
| --- | ---: | ---: |
| 1 | 8.732 s | 6.81× |
| 2 | 5.332 s | 11.15× |
| 4 | 3.781 s | 15.73× |
| 6 | 3.440 s | **17.29×** |
| 8 | 3.917 s | 15.18× |

Raw results, model identity, source-sample SHA256, first inference, and model
loading times are in [cpu-benchmark.json](cpu-benchmark.json). The reported speed
uses the slower of two resident runs. A separate six-thread run pinned to one
logical CPU per physical core reached 16.67×; affinity was not adopted.

The initial short-clip test with four threads measured 0.618 s first inference
and 0.513 / 0.411 s resident inference for 7.435 seconds of speech. These measure
file transcription, excluding microphone finalization, IPC, clipboard setup,
and app handling of the synthetic paste.

The app defaults to available CPU parallelism capped at six threads, based on
this machine's sweep. It can be overridden in configuration or with `--threads`.
**The benchmark still exits with failure below 20×.** No threshold was relaxed.

To reproduce the one-minute fixture after `make model`:

```sh
python3 - <<'PY'
import wave
source = 'models/parakeet-tdt-0.6b-v2-int8/test_wavs/0.wav'
with wave.open(source, 'rb') as audio:
    params = audio.getparams()
    frames = audio.readframes(audio.getnframes())
with wave.open('/tmp/just-speak-minute.wav', 'wb') as output:
    output.setparams(params)
    output.writeframes(frames * 8)
PY
target/release/just-speak --model-dir models/parakeet-tdt-0.6b-v2-int8 \
  --threads 6 benchmark /tmp/just-speak-minute.wav --expected-text 'old portrait' --json
```

## Functional checks

The ordinary Rust suite verifies WAV validity, short quiet speech, cleanup and
child reaping, socket framing, single-instance behavior, invalid configuration,
window identity, escaped compositor arguments, balanced key events, and
cancellation during clipboard setup. The separately enabled real-model test
loads the model, recognizes its sample, and reuses the resident engine.

The [daemon smoke test](../scripts/smoke-test.py) passed all six scenario groups
using the real model and fake capture/desktop helpers in isolated XDG directories:

1. Model loading, watch's initial state, and rejection of a second daemon.
2. Repeated start does not create a second recorder; release produces the expected
   transcript, copies it, and requests exactly one simulated paste.
3. Recording cancellation cleans audio; status and cancellation keep working
   after an invalid config edit; idle stop/cancel remain harmless.
4. Cancellation of an in-flight longer inference prevents later copy or paste.
5. A deliberately delayed clipboard handshake does not block status or cancel,
   and canceled output never requests a subsequent paste.
6. Watch transitions are delivered; SIGTERM during recording removes the socket
   and temporary audio and reaps the recorder.

Standalone QML and the Omarchy widget were loaded with the actual Quickshell
runtime and a synthetic loading/idle/recording/transcribing/error status feed.
Shell/Lua syntax, unit-file syntax, manifest entry points, and staging the
installer/package into temporary directories were checked. Read-only live
Hyprland checks confirmed the Lua functions used for safe key dispatch.

The [native PipeWire test](../scripts/test-pipewire.py) starts an isolated server
without hardware discovery or a session manager and manually connects a
synthetic tone source to the actual `pw-record`. It captured 0.608 seconds of
valid PCM16 mono 16 kHz WAV and stopped in approximately 3 ms. This uncovered
PipeWire 1.6.8's normal SIGINT exit code of 1. The recorder now accepts that code
only after sending its own stop signal and validating the finalized WAV; unit
regressions separately reject a recorder that had already failed before stop.

## CUDA experiment

The [separate CUDA benchmark](gpu-benchmark.md) used the actual GTX 1650 with
the official FP16 model and GPU runtime. Its one-minute resident inference
reached approximately 14× real time, and the long transcript included extra
unknown-token markers. GPU utilization and memory use confirmed CUDA execution.
This result did not justify switching the app's verified CPU backend. CUDA
support is not exposed as an unverified configuration option in this release.

## Scope of validation

No real microphone conversation was captured, no transcript was pasted into
the user's active applications, and no hotkeys, services, or bar widgets were
activated. End-to-end behavior across actual browser, editor, terminal, and
XWayland applications still needs interactive acceptance testing. A background
process successfully requesting a paste is not proof that every application
accepted it.

The UI and package integration are provided for installation; the repository's
downloaded model and build outputs are intentionally excluded from Git.
This initial build uses CPU inference. See [third-party notices](../THIRD_PARTY_NOTICES.md)
for the distinction between the original source license and linked runtime.
