# Installing and updating JustSpeak

JustSpeak has one Linux x86_64 release containing the recognition daemon, the shared GTK settings/history window, and the optional Omarchy bar interface. Models are downloaded separately. The release installer uses the latest stable release of `zyuapp/just-speak-linux` unless `--version 0.2.0` selects a particular version.

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/zyuapp/just-speak-linux/main/scripts/install-release.sh | bash
```

Run this as your desktop user. The default installation prefix is `~/.local`; set `JUST_SPEAK_PREFIX` or pass `--prefix` for another user-owned prefix. The installer downloads the configured speech model, writes and enables the user service, and installs the GTK launcher. It enables the bar plugin only when Omarchy is present. It leaves existing shortcuts alone. On supported Omarchy, explicitly pass `--bind-f10` to configure F10 through JustSpeak's conflict checks and validated binding editor.

For a nondefault install prefix, use `--no-bar` unless the Omarchy shell has
`JUST_SPEAK_BIN` set to that prefix's stable `bin/just-speak` path. The GTK
launcher uses the installed absolute path; the optional bar otherwise prefers
`~/.local/bin/just-speak`.

Dependencies are not installed automatically. On Ubuntu 22.04+, the corresponding package command is:

```sh
sudo apt install curl python3 pipewire-bin wireplumber wl-clipboard gjs gir1.2-gtk-4.0
```

Ubuntu's GTK introspection package is [`gir1.2-gtk-4.0`](https://packages.ubuntu.com/jammy-updates/gir1.2-gtk-4.0). A functioning PipeWire desktop session is required. The GTK window is shared across desktops; Ubuntu GNOME hold-to-talk and automatic paste remain experimental and unverified. The installer does not install Hyprland or Quickshell on other desktops.

Installer options:

- `--no-model`: skip model download and preserve the existing model configuration.
- `--no-start`: write integration files and leave the daemon stopped.
- `--no-bar`: skip Omarchy plugin installation.
- `--archive /absolute/path/release.tar.gz --version 0.2.0`: install an explicitly trusted local archive without fetching a release. Combine with `--no-model --no-start --no-bar` for an offline local installation.
- `--help`: show all options.

The installer downloads and validates everything before stopping an existing idle service. A recording, transcription, or model load blocks installation. It refuses to replace a daemon running outside its systemd user service. If applying files fails, the previous installation remains active and a previously running service is started again. After a successful file installation, a later model-download or desktop-integration failure is reported; rerunning the installer can finish these steps.

## Updates from the app

Use the settings window's update controls or these commands:

```sh
just-speak update check
just-speak update install
```

Checks read GitHub stable-release metadata. Installing is explicit; checking does not download or replace the application. Installation refuses equal versions, downgrades, prereleases, unsupported architectures, and executables outside their configured local installation. `/usr/bin` and other system installations must use their package manager.

Downloads and archive validation happen before the daemon briefly blocks new recordings. The updater then switches one release pointer, making the binary, GTK files, Omarchy files, and license notices change together. The caller restarts the daemon and refreshes the Omarchy plugin after success. It does not replace models, configuration, history, or hotkeys.

The updater verifies SHA256 against `SHA256SUMS` from the same GitHub release, fetched over HTTPS. This detects a corrupt or mismatched download. **These checksums are not an independent cryptographic signature**: the trust boundary includes the GitHub repository, its release publishers, and HTTPS. An account capable of replacing both assets can also replace their checksums. The curl installer itself is also code from that publisher; download and inspect it first if preferred.

Archive validation accepts only an explicit payload layout and regular files, requires an exact manifest and all application files, rejects traversal and links, and imposes compressed, expanded, entry-count, and per-file limits. Applying an update uses a local exclusive lock. The old release remains available if a filesystem operation fails. There is currently no automatic rollback based on the new daemon's health.

## Installation layout and recovery

The installation contains stable links such as `~/.local/bin/just-speak` and `~/.local/share/just-speak/gtk`. These all point through:

```text
~/.local/share/just-speak/updates/
  current -> releases/v0.2.0-<unique suffix>
  previous -> releases/<previous release>
  releases/
  migration-backup-...     # Original files retained when migrating v0.1
```

Old releases are retained; they are not automatically deleted. Existing v0.1 files are copied into a legacy release and their original paths are retained as migration backups. A restart is required after manually switching `current`.

To restore the previous application after a bad release, finish or cancel any active recording, then run:

```sh
systemctl --user stop just-speak.service
python3 - <<'PY'
import os, pathlib, tempfile
prefix = pathlib.Path(os.environ.get('JUST_SPEAK_PREFIX', str(pathlib.Path.home() / '.local')))
updates = prefix / 'share/just-speak/updates'
target = pathlib.Path(os.readlink(updates / 'previous'))
if target.is_absolute() or len(target.parts) != 2 or target.parts[0] != 'releases' or target.parts[1] in ('.', '..') or not (updates / target / 'bin/just-speak').is_file():
    raise SystemExit('Previous release pointer is missing or invalid; nothing changed.')
with tempfile.TemporaryDirectory(prefix='.rollback-', dir=updates) as temporary:
    link = pathlib.Path(temporary) / 'current'
    link.symlink_to(target)
    os.replace(link, updates / 'current')
print('Previous release restored.')
PY
systemctl --user start just-speak.service
```

On Omarchy, run `omarchy-shell shell rescanPlugins` after restoring the release. Reopen the GTK window. If an older installation used the standalone overlay service, restart that service too. Models and user data remain in place throughout rollback.

## Producing a release

The workflow in `.github/workflows/release.yml` runs for manual dispatch and `v*` tags. It has read-only repository permissions and produces [GitHub Actions artifacts](https://docs.github.com/en/actions/tutorials/store-and-share-data); it does not publish a release automatically.

The workflow builds on Ubuntu 22.04 with Rust 1.88, checks the `GLIBC_2.35` and `GLIBCXX_3.4.30` ABI ceiling, runs Rust tests and Clippy, and exercises both the model test and release executable against the official model's speech fixture. The native runtime is built with TTS disabled. The package creator scans the final binary for eSpeak/Piper symbols, includes native and Rust license texts, and packages source from the exact clean commit.

Locally:

```sh
scripts/build-runtime.sh /tmp/just-speak-native
export SHERPA_ONNX_LIB_DIR=/tmp/just-speak-native/lib
cargo test --locked
cargo build --release --locked
scripts/create-release.sh ./dist
```

A local build inherits its host's ABI requirements. For example, an Arch-built binary can require a newer glibc than Ubuntu provides. Public portable assets must come from the verified Ubuntu baseline build, followed by testing that exact artifact on Omarchy. Before publication, maintainers also review the public source and license inventory. The release assets are:

- `just-speak-linux-vVERSION-linux-x86_64.tar.gz`
- `just-speak-linux-vVERSION-source.tar.gz`
- `install-release.sh`
- `SHA256SUMS`

The speech model is not bundled in the binary archive. Its explicit downloader verifies the pinned upstream archive separately and preserves its attribution information.


## Omarchy interface after upgrades

Starting with 0.2.3, a successful OTA upgrade restarts the Omarchy shell when
JustSpeak's bar plugin is installed and enabled. This briefly reloads the bar
and its popups so cached QML cannot survive the upgrade. The standalone GTK
window still needs to be closed and reopened when its own code changes.
The release installer uses the same refresh command after enabling the plugin.
Other desktops and disabled JustSpeak bar integrations are skipped.

The updater runs independently of the popup that launched it, so reloading the
shell does not interrupt installation. Omarchy's supported restart command
preserves its lock-screen safeguards. A refused or failed restart is reported
as an installed update needing interface refresh; it is not silently ignored.
After unlocking or resolving the error, retry without reinstalling:

```sh
just-speak update refresh-ui
```

Older updaters, including the local 0.2.2 development build, predate this fix.
Use the 0.2.3 release installer for that transition, or run the refresh command
once after the first OTA upgrade if the popup remains stale.
Subsequent upgrades use the corrected automatic refresh path.
