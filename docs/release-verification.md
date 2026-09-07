# Version 0.2 release verification

Verification is performed with isolated fixtures wherever recording, clipboard,
or desktop mutations would otherwise affect the user's session. GNOME/Ubuntu
interactive desktop behavior is not included in the Omarchy verification scope.

## Local result (September 7, 2026)

All **52 Rust tests passed**, including the real-model regression, with Rust
1.88.0 and the corrected ASR-only runtime. Strict Clippy and formatting checks
passed. All seven real-model daemon smoke groups passed. The isolated PipeWire
recording test passed. GTK construction/action/visual checks and the real
Quickshell popup with synthetic data passed. The release installer passed fresh
install, reinstall, special-character prefix and rejected-version checks under
isolated HOME/XDG directories without changing live services.

Source and historical-blob scans found no credential patterns. Model files,
recordings, personal configuration and build artifacts are excluded from Git.

## Checks

- Rust unit tests cover waveform validation, cancellation generations, private
  socket permissions, history limits/persistence, microphone identities, mute
  recovery, shortcut conflict handling, and release archive validation/atomic
  installation.
- The real-model smoke suite supplies synthetic `pw-record`, `hyprctl`, and
  `wl-copy` helpers. It verifies transcription, retained history, preference
  changes, duplicate start suppression, asynchronous recorder lifetime,
  cancellation during inference and clipboard handshakes, and shutdown cleanup.
- A separate isolated PipeWire server verifies real `pw-record` behavior without
  recording a physical microphone or playing sound through a physical speaker.
- GTK and Quickshell use synthetic menu/history/status fixtures for construction,
  action routing, capability gating and update controls. Actual GNOME permission,
  shortcut and paste behavior remains unverified.
- The release builder checks the executable version and absence of eSpeak/Piper
  symbols, includes native and Rust dependency notices, and packages the committed
  source. Release archives have a complete file manifest and SHA256SUMS.

## Native-build corrections caught before release

The general-purpose upstream static runtime included unused speech-synthesis
components. The public build disables TTS and uses a vendored linker that refuses
that general-purpose runtime.

The initial ASR-only local build exposed a C++ dual-ABI mismatch with the pinned
ONNX Runtime archive. The runtime builder now explicitly matches ONNX Runtime's
old libstdc++ string ABI, and its verification marker records that requirement.
Real-model testing is required after linking; unit tests alone do not prove that
native model initialization works.

Moving microphone startup off the service event loop also exposed Linux's
thread-specific parent-death signal behavior. The recorder now retains its
spawning thread until the child is reaped, preserving crash cleanup without
prematurely stopping recording when a short-lived startup worker exits.

## Portability

The development machine's compiler produced a binary requiring GLIBC 2.43.
Public portable artifacts therefore use the Ubuntu release workflow baseline;
the locally built artifact is not evidence of compatibility with older Ubuntu.
A shared binary and GTK interface do not establish tested GNOME global shortcuts
or automatic paste. See [desktop support](desktop-support.md).
