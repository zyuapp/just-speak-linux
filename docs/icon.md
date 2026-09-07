# JustSpeak icon

The speech bubble with three sound bars is original JustSpeak artwork, licensed
under the repository's MIT license. It uses no third-party artwork or icon font.

`ui/just-speak.svg` is the full-color application icon. `ui/JustSpeakIcon.qml`
draws its simplified monochrome counterpart for the Omarchy bar, following the
bar's foreground color. Recording and transcribing retain their dot and spinner.
The executable embeds the SVG; the release installer and installed application
register the launcher icon as `just-speak`.

Version 0.2.5 shipped the SVG as an archive entry that older OTA validators
rejected. Version 0.2.6 restores the original payload layout: the bar artwork
remains QML and the launcher SVG travels inside the executable. Existing versions
can upgrade directly through the app. On daemon startup or window launch, the
installed binary writes the embedded icon atomically and updates the legacy
microphone launcher icon while preserving its command and other settings. This
also replaces v0.2.5 icon symlinks, which would otherwise become dangling.
