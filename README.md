# JustSpeak for Linux

Offline voice dictation with a resident Rust speech service, a shared GTK4 window,
and an optional Omarchy bar menu. On Omarchy, hold **F10**, speak, then release to
transcribe and paste. **Escape** cancels. Existing Voxtype **F9** bindings are
preserved.

One repository serves both desktop interfaces. Omarchy 4 / Hyprland 0.56 Lua is the
verified desktop integration. **Ubuntu GNOME/Wayland integration is experimental**:
the shared window and backend are intended to work there, but global hold-to-talk
and automatic paste have not been tested on GNOME. See [desktop support](docs/desktop-support.md).

## Install a release

Run as your normal desktop user:

```sh
curl -fsSL https://raw.githubusercontent.com/zyuapp/just-speak-linux/main/scripts/install-release.sh | bash
```

The installer downloads the latest Linux x86_64 release, verifies its SHA256,
installs under `~/.local`, downloads the separately pinned English speech model,
and enables the user service. It activates the optional bar integration only on
Omarchy. It does not install system dependencies or silently replace your shortcuts.
Review the [installer](scripts/install-release.sh) before running it if preferred.
For supported options, download it and run `bash install-release.sh --help`.

On Omarchy, click **Record shortcut**, press **F10** (or your preferred
combination), then click **Save**. There is no shortcut name to type. Advanced
users can still configure it from the command line:

```sh
~/.local/bin/just-speak shortcut set F10
```

The editor checks existing bindings, backs up your configuration, and rejects
conflicts. Omarchy's existing F9 Voxtype shortcut stays intact. On GNOME, use the
window's Start/Stop controls and manual clipboard paste while native integration
is being developed.

Requirements: Linux x86_64, a user systemd session, PipeWire (`pw-record`,
`pw-dump`, `pw-play`), WirePlumber (`wpctl`), `wl-clipboard`, Bash, curl, Python 3,
coreutils and util-linux. The shared window and shortcut recorder need **GJS and GTK4 introspection**.
The Omarchy bar uses its installed Quickshell shell. The installer reports missing
dependencies rather than invoking a package manager with elevated privileges.

## Use the app

Open **JustSpeak** from your application launcher, run `just-speak window`, or
click its Omarchy bar icon. The service keeps the speech model loaded after the
window closes.

- Choose a microphone by its stable PipeWire name, or follow the system default.
- Review the last ten transcripts, copy or paste an entry, or clear history.
- Record a shortcut by pressing its keys on supported Hyprland Lua desktops.
  Review the captured combination before saving; Escape cancels and conflicts
  leave the existing shortcut intact.
- Toggle recording sounds, output muting, automatic paste, history, and update checks.
- Check for and install application updates; restart or quit the service.

The recording indicator shows listening, transcription, cancellation and errors.
Capture starts before the optional start sound so delayed playback cannot cut
off early speech. Output muting follows the sound attempt, even if it fails,
and restores the original sink when recording ends; recovery also checks for
an interrupted previous service. User
volume/mute changes and replaced audio devices are not blindly overwritten.

```sh
just-speak status
just-speak doctor
just-speak start
just-speak stop
just-speak cancel
just-speak update check --json
just-speak update install
```

`just-speak launch` starts the installed user service; `just-speak restart` reloads
it when idle. `just-speak quit` stops it. The app launcher/window remains available
to start it again.

## Privacy and behavior

Transcription uses English **Parakeet TDT 0.6B v2 INT8**, through an ASR-only
sherpa-onnx CPU runtime. There is no cloud transcription, account, telemetry, or
CUDA dependency. The one-time model download is about 483 MB; installed model
files occupy about 661 MB and are separate from application updates.

Audio exists only in private temporary WAV files during recording/transcription.
Normal completion, cancellation and shutdown delete them; a hard crash can leave
private temporary files until system cleanup. Enabled history stores the last ten
completed transcripts locally at `~/.local/state/just-speak/history.json`, with
private file permissions. Turning history off stops new retention; **Clear
history** deletes existing entries. History and clipboard contents may contain
sensitive text, so use these controls as appropriate.

Model downloads contact the upstream model host. Enabled update checks contact
GitHub on UI startup when idle and at most every six hours per interface; manual
checks run immediately. Checks transmit no recordings or transcripts. Application
updates require an explicit Install action. Checksums detect corrupt/tampered
payloads relative to the trusted GitHub release metadata; they are not an
independent publisher signature. See [updates and rollback](docs/updates.md).

On Hyprland, paste is guarded by the original window identity. If focus changes,
the text is copied and automatic paste is skipped. A canceled inference result is
discarded; an already delivered clipboard write or keystroke cannot be retracted.
Recordings have a configurable 1–120 second limit.

## Settings

Optional `${XDG_CONFIG_HOME:-~/.config}/just-speak/config.toml`:

```toml
num_threads = 6
paste = true
max_recording_seconds = 120
shortcut = "F10"
sound_feedback = true
mute_while_recording = true
history_enabled = true
auto_check_updates = true
# model_dir = "/absolute/path/to/parakeet-tdt-0.6b-v2-int8"
# input = "stable PipeWire node.name"
```

Menu changes take effect immediately when idle. Restart after manually editing
configuration. `--model-dir` and `--threads` override a newly launched process;
they do not reconfigure an already-running service.

## Build and verify

Requires Rust 1.88+, a C/C++ toolchain, CMake, Make, curl, tar, Python 3, and Lua for
Hyprland integration tests. Build the pinned native runtime without unused TTS
components, then the app:

```sh
./scripts/build-runtime.sh "$PWD/native"
export SHERPA_ONNX_LIB_DIR="$PWD/native/lib"
make build
make check
make model
make smoke
```

The runtime builder verifies pinned downloads and assembles their redistribution
notices. The vendored Rust linker refuses the general-purpose upstream runtime.
Release packaging includes application source and the native/Rust license bundle.

File transcription does not touch the clipboard:

```sh
just-speak transcribe /path/to/mono-16khz-pcm16.wav
```

Measured resident CPU inference on a Ryzen 5 5600H reached roughly **17× real
time** for a 59-second fixture. The aspirational 20× benchmark gate remains
unmet on that hardware. See [verification](docs/verification.md) and the
[CUDA experiment](docs/gpu-benchmark.md); GPU support is not shipped.

Original application source is MIT licensed. Dependencies retain their own
licenses; see [third-party notices](THIRD_PARTY_NOTICES.md) and the license bundle
included in each binary release.
