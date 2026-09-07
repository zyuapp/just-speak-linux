# Desktop integration

These files target Omarchy 4, Hyprland 0.56's Lua config, and Quickshell 0.3.
Installing assets does not download a model, enable services, change your hotkeys,
or change the Omarchy bar. Complete the model setup in the project README first.

## Install a local build

From the repository, run `cargo build --release --locked`, then
`./scripts/install.sh`. This installs under `~/.local` and writes user units under
`${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user`. Set `JUST_SPEAK_PREFIX` to change
the install prefix. Ensure its `bin` directory is on your desktop session's PATH.

Once the model is installed, start the daemon:

```sh
systemctl --user daemon-reload
systemctl --user enable --now just-speak.service
just-speak status
```

The units use `graphical-session.target`; Omarchy's UWSM session imports
`WAYLAND_DISPLAY` and `HYPRLAND_INSTANCE_SIGNATURE` into the user manager before
starting that target. For a manually launched compositor, import those variables
from a terminal in that session before starting the service:

```sh
systemctl --user import-environment WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
```

## Hold F10 to speak

Review `hyprland.lua`, then add its contents to `~/.config/hypr/bindings.lua`.
It assigns **F10 to JustSpeak and leaves Voxtype on F9**. Press starts recording;
release transcribes and pastes. Escape cancels and still reaches the application,
including while recording; idle cancellation is a no-op. Escape uses Hyprland's
[`non_consuming` flag](https://wiki.hypr.land/configuring/core/binds/flags/).

Before editing, back up your bindings file and check `omarchy menu keybindings
--print` for conflicting custom bindings. After editing, run:

```sh
hyprctl reload
hyprctl configerrors
```

## Choose one interface

The Omarchy widget includes the recording overlay. Use the widget **or** the
standalone overlay service to avoid duplicate overlays.

For the Omarchy bar, copy the installed UI directory into the user plugin path:

```sh
mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins/local.just-speak"
cp -r "$HOME/.local/share/just-speak/ui/." "${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins/local.just-speak/"
omarchy-shell shell rescanPlugins
omarchy plugin enable local.just-speak --section right
```

With the Arch package, use `/usr/share/just-speak/ui/.` as the copy source instead.
The widget shows readiness and errors on hover. Left click starts/stops dictation;
right click cancels. Its small overlay never takes keyboard focus and lets mouse
clicks pass through. The UI reconnects automatically when the daemon restarts.

For the standalone overlay instead:

```sh
systemctl --user enable --now just-speak-overlay.service
```

During development it can also run directly with `quickshell --no-duplicate --path
ui`. Set `JUST_SPEAK_BIN` to an absolute binary path if the build is not on PATH.

## Arch package

After committing a snapshot, create its local source archive from the repo root:

```sh
git archive --format=tar.gz --prefix=just-speak-linux-0.1.0/ --output=packaging/just-speak-linux-0.1.0.tar.gz HEAD
cd packaging
makepkg -si
```

The PKGBUILD builds that local archive and runs the Rust tests. It has no package
install hooks and does not fetch model weights. Model download remains an
explicit user action. Replace the local `SKIP` checksum with a fixed checksum
before publishing release artifacts.

The first build needs network access: `cargo fetch` downloads Rust crates, and
sherpa-onnx's build script separately downloads its native static runtime.
`cargo build --frozen` fixes Cargo dependency resolution; it does not prevent
that upstream native download. Installed binaries use the linked static runtime.

## Remove the integration

Disable the widget with `omarchy plugin disable local.just-speak` and remove its
directory from `~/.config/omarchy/plugins`. Disable services with `systemctl --user
disable --now just-speak-overlay.service just-speak.service`. Remove the added
JustSpeak hotkey block, then run
`hyprctl reload` and `hyprctl configerrors`.

For a local install, remove only the installed `bin/just-speak`,
`share/just-speak`, and the two user unit files, then run `systemctl --user
daemon-reload`. For the Arch package, remove `just-speak-linux` with your package
manager. Models and user settings remain available for a later reinstall.
