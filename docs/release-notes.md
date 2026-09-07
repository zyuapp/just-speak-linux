Version 0.2.8 records your hold-to-talk shortcut directly in the Omarchy bar
popup. Record, preview, Save and Cancel stay together without opening a settings
window.

The recorder waits for desktop shortcut protection, keeps completed previews
across focus changes, and waits for held keys to release before saving or
canceling. Occupied shortcuts show an error in place so you can record again.
Keyboard navigation, keymap lookup failures, and recording timeouts are covered
by regression checks. Function keys also work with virtual keyboards.

This release retains the existing GTK recorder and OTA compatibility with older
clients. Existing models, history, settings and shortcuts are preserved.
