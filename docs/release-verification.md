# Version 0.2 release verification

## Installed shortcut lookup (0.2.10)

The 0.2.8/0.2.9 popup could not locate its keymap helper after Omarchy loaded the
plugin through symlinks and Quickshell virtual URLs. The original popup fixture
used sibling UI/helper directories, so it missed this installation failure.
Changing that fixture to load the UI via a symlink outside the payload reproduced
the error before the fix.

The popup now calls `shortcut resolve-key` on its existing executable. That
command embeds the GJS script, runs before configuration/service access, and
replaces its process with GJS so existing cancellation/deadline behavior holds.
The fixture now verifies actual Super+F11 capture, release gating, the Save
button, and the saved setting. A separate CLI regression copies the binary to
an unrelated directory without frontend files and verifies embedded-script
execution, invalid-settings independence, exit status and argument bounds.
Local checks passed all 60 ordinary Rust tests, strict Clippy, formatting, the
26 QtTest results, the keymap fixtures, and timeout/retry through the new CLI
launcher. The final release binary passed the relocated executable check and
real Super+F11 capture/Save in the symlinked popup fixture.

## First-run speech model setup (0.2.9)

The shared GTK window offers an explicit model download, stage progress, and
retry after failure. The service owns the download and loads the model when it
finishes, so closing the window does not interrupt setup. The Omarchy popup
links to the same screen. Existing invalid model directories are preserved.

Local checks passed 60 ordinary Rust tests, strict Clippy, formatting, shell
syntax, and GTK setup states/action gates. An isolated daemon test verified
missing-model detection, explicit consent to download, independent client
connections, duplicate-request rejection, failed downloads and retries, and
automatic loading of the existing real model after simulated delivery. The
real-model dictation smoke suite passed on rerun after one transient CLI-status
failure in its audio-feedback check. Tests use synthetic downloads and desktop
helpers; they do not access the user's microphone or clipboard.

The v0.2.8 release job failed before building because Ubuntu's Qt offscreen
platform plugin was absent. Version 0.2.9 explicitly installs `qt6-qpa-plugins`
and retains the inline shortcut capture changes and their tests. The tagged
workflow gates publication on Ubuntu GTK, native/ABI checks, first-run setup,
real-model dictation, and acceptance by the published v0.2.3 updater.

## Inline Omarchy shortcut recorder (0.2.8)

The Omarchy popup now captures, previews, saves and cancels shortcuts inline.
Quickshell's compositor-granted ShortcutInhibitor protects the existing panel;
a GJS helper reads the keyboard layout without creating a GTK window. Save
still uses the existing backend conflict checks.

Local verification passed all 58 ordinary Rust tests, strict Clippy, formatting,
the existing GTK capture tests, the keymap translation fixtures, and all 26
QtTest results for inline capture. Cases include modifier combinations and
release order, pre-held and duplicate modifiers, repeats, shifted punctuation,
invalid keys, AltGr/keypad rejection, stale replies, focus interruption,
preview preservation, save failures, keys pressed during saves, and keyboard
navigation. The release workflow runs the new capture/keymap tests as well.

An isolated copy of the actual Omarchy panel passed capability gates, inline
conflict/retry/save routing, Escape, and outside-dismissal release gating. The
live Hyprland variant additionally verified real shortcut inhibition, focus
loss/recovery, hardware-code translation, and real F35/F34 events delivered
through a virtual keyboard. Those keys were confirmed unbound before injection;
all saves used a fake backend. The live test caught and fixed translation of
function keys from a virtual keyboard whose keymap differs from a new client's.
A captured image confirmed the recorder controls remain inside the popup.

Separate hidden fixtures passed denied protection, malformed keymap output,
a hung keymap helper followed by retry, and the real 30-second capture timeout
while a key was held. They did not modify user shortcuts, microphone input,
clipboard contents or running services. GNOME behavior is outside this change.
Layout-ambiguous character keys fail with an explanation rather than guessing;
AltGr and numeric-keypad shortcuts are explicitly unsupported in this recorder.

Publication remains subject to the tagged release workflow's Ubuntu GTK,
portable ABI, real-model and older-client OTA acceptance checks.

## Modifier-only shortcut rejection (0.2.4)

The recorder and backend reject new modifier-only bindings, including Right Alt
and combinations containing only modifiers. Older configurations remain loadable
so users can replace their shortcut without losing access to settings.

All 57 ordinary Rust tests and the synthetic GTK capture sequences passed locally,
including rejection, retry, valid Alt chords, and legacy configuration loading.
Strict Clippy and formatting checks passed. Local GTK window lifecycle checks
require unavailable Xvfb; the release workflow now runs those Ubuntu GTK checks
alongside its existing ABI and real-model acceptance checks before publishing
tagged releases. Publication is asynchronous; these local results do not claim
that the portable build has completed.

## Upgrade interface refresh (0.2.3)

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

The combined release preserves the separately completed capture-start fix.
All 55 ordinary Rust tests and all eight real-model smoke groups passed locally.
A CLI fixture confirmed that only the matching installation's plugin is
refreshed. A real, harmless transient-unit test confirmed that journal output
survives loss of the updater's launcher and collectors, while pipe output fails.
The smoke suite now waits for synthetic audio readiness before exercising
inference cancellation, removing a scheduling race found in CI.

The final [portable build](https://github.com/zyuapp/just-speak-linux/actions/runs/34152272756)
passed for `500792cb77b000cd3c397f3b459c5f17e5d884e3`, including real-model
acceptance. Every asset checksum and all 77 source files matched that commit.
[Version 0.2.3](https://github.com/zyuapp/just-speak-linux/releases/tag/v0.2.3)
was published. At the user's request, it was not installed and the app was not
restarted; the user is testing the OTA transition themselves.

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
