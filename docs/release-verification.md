# Version 0.2 release verification

## Upgrade interface refresh (0.2.2)

A live 0.2.1 popup remained stale after a successful OTA file switch and plugin
rescan. Restarting the Omarchy shell loaded the correct Record shortcut button.
A separate single-engine test also showed that release-specific QML filenames
could fail against cached directory metadata after an atomic symlink switch.

The final fix uses Omarchy's supported shell restart after installation when
this installation's JustSpeak plugin is enabled, then checks shell readiness.
The release installer calls the same helper. Tests cover enabled, absent, and
disabled integrations; restart refusal; malformed registry output; and failed
readiness. Update service output goes to the journal so closing the originating
popup cannot break its output pipe. The refresh command can be retried without
reinstalling if a locked session or another error prevents the shell restart.

## Shortcut recorder (0.2.1)

The shortcut recorder replaces free-text editing in both interfaces. The GTK
dialog waits for compositor shortcut inhibition before accepting a key, records
physical key combinations, and requires an explicit Save. Existing backend
conflict checks remain responsible for rejecting occupied shortcuts.

Local checks passed for the 51 ordinary Rust tests, strict Clippy, formatting,
shell syntax, and synthetic GTK key sequences. These tests cover modifiers,
shifted punctuation, repeats, preview state, and key-release gating. The QML
popup was checked with synthetic data and harmless helper programs: the button
respects busy/desktop capability states and closes the popup before launching
the shared recorder. Hidden GTK lifecycle tests cover denied protection, focus
loss, conflicts, cancellation, keys held during saving, pre-held modifiers,
keyboard navigation, and timeout cleanup. On live Hyprland, the compositor
granted inhibition, unbound F35 was captured, holding unbound F34 disabled Save,
and the final release and synthetic save restored inhibition. These checks do
not record microphone input or alter the user's existing bindings.

An isolated single-instance launch check uses a synthetic backend: a second
request returns promptly and opens the recorder in the original GTK process.
Action delivery avoids retaining a GJS command-line object, which otherwise
delayed the launching process until garbage collection. Recorder creation waits
for the main window to gain focus so its initial activation cannot cancel the
dialog. Ubuntu's GTK 4.6 also required explicitly clearing the transient parent
before destroying the dialog; a debugger trace confirmed the stale-parent
cleanup path during GJS shutdown.

The final [portable release build](https://github.com/zyuapp/just-speak-linux/actions/runs/34149081570)
and [Ubuntu 22.04 GTK check](https://github.com/zyuapp/just-speak-linux/actions/runs/34149083560)
passed for `6856248b91aca11efddcff80720d316680e8f1b5`. All asset checksums,
the complete payload manifest, and all 76 source archive files matched the
audited commit. The downloaded portable binary also recognized the official
speech fixture on Omarchy. Cold and existing-window recorder requests both
returned promptly and kept the same GTK process in isolated live checks.

[Version 0.2.1](https://github.com/zyuapp/just-speak-linux/releases/tag/v0.2.1)
was published and installed through the existing 0.2.0 application's OTA path.
The update detected the newer release, completed successfully, restarted the
resident service with its model ready, and then reported 0.2.1 as up to date.
The user's existing `SUPER + F10` binding and separate F9 Voxtype bindings were
preserved, with no Hyprland configuration errors. The installed GTK window
opened with an empty error log and the recorder module present. Isolated
accessibility service tests temporarily disrupted the shared accessibility
socket; restarting that service restored it before the final window check.

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

## Published artifact acceptance

The [Ubuntu baseline release build](https://github.com/zyuapp/just-speak-linux/actions/runs/34096127723)
passed for commit `a5c6e71f3728e79e64bf3a0d14b60c9a5a6dca17`.
Its dynamic ABI maxima are GLIBC 2.34 and GLIBCXX 3.4.30. The separate
[Ubuntu 22.04 GTK compatibility run](https://github.com/zyuapp/just-speak-linux/actions/runs/34096273522)
also passed with synthetic controls under Xvfb; this does not constitute a
GNOME/Wayland shortcut, microphone-permission or paste test.

The exact downloaded binary passed all seven real-model daemon smoke groups
again on the Omarchy development machine. Its source archive matched all 73
files of the audited commit, and every release asset matched SHA256SUMS.
[Version 0.2.0](https://github.com/zyuapp/just-speak-linux/releases/tag/v0.2.0)
was then published and installed through its public release installer.

Live acceptance confirmed the enabled, active user service with its model ready,
reuse of the existing model, available microphone enumeration, the persistent
bar plugin, and a mapped GTK window. The new shortcut editor successfully saved
F10 with no Hyprland configuration errors; the original F9 Voxtype press/release
bindings remained present. The duplicate standalone overlay service was disabled.
The installed update check correctly reported current/latest 0.2.0 with no newer
release available. These checks did not record a physical conversation or paste
text into the user's applications.
