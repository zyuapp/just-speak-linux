# Shared desktop window

The window uses GJS and GTK4 and talks to the same Rust service as the optional
Omarchy bar plugin. It does not depend on Omarchy, Quickshell, or libadwaita.

Run `just-speak window` after installation, or `gjs -m gtk/main.js` from a
checkout. `JUST_SPEAK_BIN` can select a development binary; otherwise the window
prefers `~/.local/bin/just-speak` and then searches PATH. Closing the window leaves
the dictation service running. Reopening the launcher activates the existing
window when it is still running.

Microphones, settings, history, and desktop capabilities come from
`just-speak menu --json`. The window polls status without blocking GTK and avoids
overlapping polls. Transcript buttons send history IDs to the service, never
transcript text in command arguments. History labels display plain text.

On Hyprland, manual recording and history paste hide the window briefly before
the service captures the target application. Finish through the configured
shortcut or reopen JustSpeak. On desktops without automatic paste, manual
Start/Finish stays visible and results go to the clipboard. Shortcut editing and
automatic paste are disabled according to the service's desktop capabilities;
GNOME behavior has not been verified.

`gjs -m gtk/main.js --smoke-test` constructs the window without showing it,
exercises synthetic history/settings and desktop capability gates, and exits.
It never records, uses the clipboard, changes settings, mutes audio, or checks
for updates. GTK still needs a display connection to initialize.

For visual checks, `--smoke-test-visible` shows the same synthetic window for
eight seconds and exits. Its backend remains disabled for all external actions.
