JustSpeak now has its own speech-bubble and sound-bar icon: warm orange in the application launcher and a theme-colored version in the Omarchy bar. Recording and transcription keep their existing status indicators. The artwork is original and covered by the project’s MIT license.

Upgrading from v0.2.4 or earlier requires the release installer because those versions’ in-app updater rejects SVG assets. Run the installer below as your desktop user; it also registers the new launcher icon.

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/zyuapp/just-speak-linux/main/scripts/install-release.sh | bash -s -- --version 0.2.5
```
