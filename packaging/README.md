# Desktop integration and local Arch packaging

JustSpeak 0.2 uses one Rust service and a shared GTK4/GJS window. Omarchy 4 with
Hyprland 0.56 Lua additionally supports a bar menu, focus-checked paste, and
shortcut editing. The GTK window provides Start/Stop and clipboard use on other
Wayland desktops; GNOME global shortcuts and automatic paste remain experimental
and unverified. See [desktop support](../docs/desktop-support.md).

## User-local installation

The [release installer](../scripts/install-release.sh) installs into `~/.local`,
downloads the separate speech model, and enables the daemon. It activates the
optional bar plugin when Omarchy is present. Run it without sudo; it reports
missing dependencies and leaves shortcut changes to an explicit action.

For a source checkout, build and verify the ASR-only runtime and Rust executable
as described in the project README, then commit the snapshot and run:

```sh
export SHERPA_ONNX_LIB_DIR="$PWD/native/lib"
./scripts/install.sh
```

The source installer creates a checked release archive and uses the same
versioned installation layout as downloaded releases. It skips model download
and service startup. Download the model and start when ready:

```sh
just-speak model download
systemctl --user daemon-reload
systemctl --user enable --now just-speak.service
just-speak window
```

Set `JUST_SPEAK_PREFIX` to use another user-owned prefix. Put its `bin` directory
on the desktop session's PATH. GTK settings require `gjs` and GTK4; capture and
optional feedback require PipeWire tools and WirePlumber.

The service follows `graphical-session.target`. Omarchy's UWSM session supplies
the compositor environment. For a manually launched Hyprland session, import it
before starting the service:

```sh
systemctl --user import-environment WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
```

## Shortcuts and controls

Open JustSpeak from the launcher or run `just-speak window`. Choose a microphone,
review the last ten transcripts, clear history, change sounds/muting and paste
preferences, manage updates, or restart/quit the service. Closing the window
leaves the service ready. On supported Hyprland desktops, choose a shortcut in
settings, or run:

```sh
just-speak shortcut set F10
```

The editor checks conflicts, saves a backup, and updates its marked binding
block. **F10 is JustSpeak's default; Voxtype on F9 is preserved.** Hold to record,
release to transcribe, and press Escape to cancel. Escape still reaches the
focused application. [hyprland.lua](hyprland.lua) is available for manual
integration; back up and inspect your bindings before applying it.

## Optional Omarchy bar

The release installer creates a symlink to the installed UI so application
updates also update the bar assets. For manual setup, first disable an older
plugin and move any existing copied `local.just-speak` directory to a backup.
Then create the symlink and enable it:

```sh
mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins"
ln -s "$HOME/.local/share/just-speak/ui" "${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins/local.just-speak"
omarchy-shell shell rescanPlugins
omarchy plugin enable local.just-speak --section right
```

For the Arch package, the symlink target is `/usr/share/just-speak/ui`. With a
custom prefix, use its `share/just-speak/ui` directory. Keep a symlink so updates
do not leave a stale copy of the interface.

Left click opens the dictation menu; right click cancels active dictation.
The menu provides capture, microphone, history, preferences, and service controls.
The bar includes the recording overlay, which does not take keyboard focus.
Use the bar or the standalone Quickshell overlay service to avoid duplicates:

```sh
systemctl --user enable --now just-speak-overlay.service
```

The Arch package supplies that optional service. The release installer installs
the shared GTK window and Omarchy plugin; manual standalone overlay setup should
point Quickshell at the installed UI directory.

## Local Arch package

This PKGBUILD builds a committed local snapshot; it is not an AUR submission.
From the repository root:

```sh
git archive --format=tar.gz --prefix=just-speak-linux-0.2.4/ --output=packaging/just-speak-linux-0.2.4.tar.gz HEAD
cd packaging
makepkg -si
```

The first build downloads locked Rust crates and hash-pinned native dependencies.
The native build disables TTS, eSpeak/Piper, GPU support, and unused interfaces.
It matches the C++ ABI required by its pinned ONNX Runtime archive.
`cargo build --frozen` builds against that explicit runtime; `check()` runs unit
tests and checks the executable for excluded TTS symbols. The ignored model test
requires a separate model download and does not run here.

The package installs the executable, GTK and optional bar assets, launcher,
systemd user units, model downloader, and collected native/Rust license texts
under `/usr`. Native notices include the exact Eigen source used by the MPL-2.0
header dependency. There are no install hooks: model download, service activation,
hotkeys, and bar activation remain user actions. Replace the local `SKIP`
checksum with the published archive's checksum before adapting this recipe for
distribution.

Use the package manager to update a `/usr` installation. In-app installation and
rollback support the versioned user-local release layout and refuse to replace
system-owned binaries. See [updates and rollback](../docs/updates.md). Use one
installation method at a time to keep binary, service, and UI paths aligned.

## Remove integration

Disable the plugin with `omarchy plugin disable local.just-speak`, then remove
only its symlink. Disable enabled units with `systemctl --user disable --now
just-speak-overlay.service just-speak.service`. Remove JustSpeak's marked hotkey
block, then run `hyprctl reload` and `hyprctl configerrors`.

Remove an Arch installation with the package manager. For a user-local release,
remove its `bin/just-speak`, `share/just-speak`, license directory, launcher, and
user service file from the installation/XDG directories, then run
`systemctl --user daemon-reload`. Settings, models, and transcript history live
separately; use Clear history before removal to delete its saved text.
