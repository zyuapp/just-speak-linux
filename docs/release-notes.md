Version 0.2.7 fixes the shortcut recorder disappearing when mouse movement
changes window focus. The dialog now pauses capture and keeps completed
shortcuts visible until you return and choose Save or Cancel. Completed previews
also remain open without the recording timeout.

Capture resumes only after the desktop grants keyboard shortcut protection and
the dialog has focus again. If focus changes while keys are still held, record
the chord again so missed key releases cannot enable an unsafe save.

This release retains OTA compatibility with v0.2.3 and the embedded JustSpeak
icon introduced in v0.2.6. Existing models, history, settings and shortcuts are
preserved. Reopen an already-running settings window after updating so it loads
the corrected recorder.
