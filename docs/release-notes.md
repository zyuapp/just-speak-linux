Version 0.2.6 restores OTA upgrades from v0.2.3 and earlier supported updaters.
If “Install update” failed with “unexpected or duplicate release file:
share/just-speak/ui/just-speak.svg”, check for updates again and install v0.2.6.
No manual installer or intermediate upgrade is needed.

The JustSpeak icon is preserved. Its launcher artwork is now embedded in the
executable so older archive validators accept the release. The installed app
registers the launcher icon on daemon startup or window launch, preserving the
existing launch command. Models, settings, history and shortcuts are retained.

Publication requires the actual published v0.2.3 updater to install the exact
new archive successfully in an isolated installation, alongside the existing
Rust, GTK, ABI and real-model checks.
