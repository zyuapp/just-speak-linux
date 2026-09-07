Version 0.2.11 fixes shutdown, cancellation and error recovery across the
JustSpeak window, Omarchy popup and background service.

- Quit now finishes service cleanup, closes the window and removes the bar
  icon. Opening JustSpeak starts it again. Quit also works during dictation
  and model setup.
- Failed preferences and microphone changes restore their previous values.
  Slow responses no longer overwrite newer actions, and failed refreshes are
  reported separately from successful saves.
- Popup action failures stay visible. Commands that cannot start or time out
  release their controls so you can retry. Failed updates also release the
  service's update lock.
- Cancellation waits for microphone and audio-feedback cleanup. An unexpected
  recorder exit now reports an error instead of leaving the app listening.
- Turning off automatic paste keeps the window's Start/Finish controls visible.
  Shortcut requests survive startup, and short transcript lists display fully.

The release adds regression checks for real window/CLI/service lifecycle,
settings rollback, delayed responses, popup errors and update retry. Local
window workflows passed on Wayland and X11; model and audio checks use isolated
fixtures. Physical microphone/paste acceptance and GNOME behavior are not
claimed by these tests.

Existing settings, models and transcript history are preserved. After updating,
close and reopen an existing JustSpeak window to load the new interface.
