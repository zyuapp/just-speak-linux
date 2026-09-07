# Shared desktop window

The window uses GJS and GTK4 and talks to the same Rust service as the optional
Omarchy bar plugin. It does not depend on Omarchy, Quickshell, or libadwaita.

Run `just-speak window` after installation, or `gjs -m gtk/main.js` from a
checkout. `JUST_SPEAK_BIN` can select a development binary; otherwise the window
prefers `~/.local/bin/just-speak` and then searches PATH. Closing the window leaves
the dictation service running. Reopening the launcher activates the existing
window when it is still running.

When no model is installed, the window offers a one-time download with the size
and source shown before starting. `just-speak model setup` asks the service to
download, verify, unpack, and load the model. Status updates carry `model_setup`
stages so closing and reopening the window preserves progress and failures can
be retried. The existing `just-speak model download` command remains a standalone
download for installers. Existing invalid model directories are preserved.

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

In the desktop window, choose **Record shortcut…** to press a shortcut instead of typing its name.
The Omarchy bar records inline in its popup using Quickshell shortcut inhibition;
it calls `just-speak shortcut resolve-key` for headless layout translation. The
CLI embeds `shortcut-keymap.js`, so lookup also works when the plugin is loaded
through symlinks or Quickshell virtual URLs. It does not launch this window.
`just-speak window --record-shortcut` opens the same recorder, including when
the desktop window is already running. The dialog waits for compositor shortcut
inhibition and focus before accepting input. It supports function keys,
modifier-key combinations, and individual left/right modifiers on release.
Escape cancels; Save (or Enter after capture) applies the preview through the
service's conflict checks. A conflict leaves the previous shortcut in place.

The dialog keeps desktop shortcuts suspended until all observed keys and held
modifiers are released, including keys pressed during the preview or save.
Cancel or a completed save waits for those releases while the dialog retains
focus. Losing focus or compositor protection pauses capture and preserves a
completed preview. Capture resumes only once both focus and protection return.
If keys were still held at interruption, the chord must be recorded again
because release events may have gone to another window. Closing the parent
still cleans up immediately. Active recording times out after 30 seconds;
paused recording and completed previews wait for the user. A pending close
with held keys asks you to release them or switch to another window. Saving is
an explicit commit: Cancel is disabled while the service finishes the request.

Recording requires the desktop to grant GTK's shortcut-inhibition request.
Compositor shortcuts configured to bypass inhibition remain controlled by the
desktop. The recorder has been checked on Hyprland; GNOME integration is still
unverified and remains disabled by the service's capability response.

`gjs -m gtk/main.js --smoke-test` constructs the window without showing it,
exercises synthetic history/settings and desktop capability gates, and exits.
It never records, uses the clipboard, changes settings, mutes audio, or checks
for updates. GTK still needs a display connection to initialize.

For visual checks, `--smoke-test-visible` shows the same synthetic window for
eight seconds and exits. Its backend remains disabled for all external actions.
Add `--smoke-model-setup` to preview the missing-model screen. The smoke checks
also cover setup progress, retry, blocked actions, and transition to dictation.

`python3 scripts/test-model-setup.py --binary target/debug/just-speak` checks an
isolated first-run daemon with synthetic download failures and retries. Add
`--model-dir models/parakeet-tdt-0.6b-v2-int8` to also verify that setup loads a
real model automatically. It never downloads or accesses desktop services.

Additional checks:

```sh
gjs -m gtk/tests/shortcut-recorder.js
xvfb-run -a dbus-run-session -- gjs -m gtk/tests/shortcut-recorder-window.js
```

The first test covers key sequences without a display. The second constructs
hidden GTK widgets and checks protection loss, conflicts, deferred cancellation
and saves, pre-held modifiers, keyboard navigation, and timeout cleanup. Neither
performs desktop actions. For an explicit live compositor check, first verify
that F34 and F35 are unbound, then run
`gjs -m gtk/tests/shortcut-recorder-window.js --compositor`. That mode briefly
shows a synthetic dialog and uses `wtype` to send those two keys; its Save action
is fake and cannot change configuration or start dictation.

API references: [GTK shortcut inhibition](https://docs.gtk.org/gdk4/method.Toplevel.inhibit_system_shortcuts.html),
[inhibition confirmation](https://docs.gtk.org/gdk4/property.Toplevel.shortcuts-inhibited.html),
[key translation](https://docs.gtk.org/gdk4/method.Display.translate_key.html),
and [local options and single-instance action delivery](https://docs.gtk.org/gio/signal.Application.handle-local-options.html).
