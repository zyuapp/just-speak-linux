Version 0.2.10 fixes shortcut capture in the installed Omarchy popup, including
Super + F11. The popup previously tried to launch a helper through a QML URL
that did not point to a real file after plugin loading.

The JustSpeak executable now contains the keymap helper and runs it directly.
Capture and Save are verified through the installed plugin's symlink layout,
including real Super + F11 input. The release checks also exercise a relocated
executable with no frontend files and invalid local settings.

Existing shortcuts, speech models, history and first-run model setup are
preserved. Shortcut recording stays inline in the Omarchy popup.
