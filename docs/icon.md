# JustSpeak icon

The speech bubble with three sound bars is original JustSpeak artwork, licensed
under the repository's MIT license. It uses no third-party artwork or icon font.

`ui/just-speak.svg` is the full-color application icon. `ui/JustSpeakIcon.qml`
draws its simplified monochrome counterpart for the Omarchy bar, following the
bar's foreground color. Recording and transcribing retain their dot and spinner.
The package and release installer register the launcher icon as `just-speak`.

For installations running v0.2.4 or earlier, use the release installer when
upgrading to a release containing this icon. Those versions' in-app updater
rejects SVG payload files. The installer validates with the incoming binary and
also registers the new launcher icon. Subsequent updates retain that registration
through a link to the active release.
