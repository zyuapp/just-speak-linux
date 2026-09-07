Version 0.2.9 adds an in-app setup screen when the speech model is missing.
Download the English model (~483 MB), follow download, verification, unpacking
and loading progress, and retry if the download fails. Setup continues when the
window closes and enables dictation automatically when the model is ready.
The Omarchy bar opens the same setup screen.

This release also includes inline shortcut recording in the Omarchy popup:
record, preview, save and cancel without opening another window. Completed
previews survive focus changes, held keys are released before saving or
canceling, and conflicts can be corrected in place.

The release fixes the missing Qt test dependency that blocked v0.2.8 from
publishing. Existing models, history, settings and shortcuts are preserved,
and the update remains compatible with older JustSpeak clients.
