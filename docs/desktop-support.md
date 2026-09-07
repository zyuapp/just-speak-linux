# Desktop support and shared architecture

JustSpeak has one Rust service and one repository. Model inference, recording,
microphone selection, private transcript history, preferences and release updates
are shared. The GTK4 window (GJS) uses the same local CLI/socket API on each
desktop. The Omarchy Quickshell bar is an optional frontend.

| Environment | Current integration | Verification |
| --- | --- | --- |
| Omarchy 4 / Hyprland 0.56 with Lua | Hold/release shortcut, guarded automatic paste, bar menu and recording overlay, GTK window | Tested on the development machine; initial F10 speech workflow confirmed by the user |
| Ubuntu GNOME / Wayland | Shared service and GTK window; manual Start/Stop and clipboard delivery intended as fallback | Experimental: no GNOME desktop acceptance test yet |
| Other desktops / X11 | No dedicated adapter | Unsupported until tested |

The Hyprland adapter reads the focused window identity, waits for clipboard
ownership, checks focus again inside the compositor, and injects a balanced paste
shortcut. GNOME does not expose those same Hyprland APIs. Compiling the service on
Ubuntu does not validate global hold-to-talk, clipboard ownership, background app
behavior, or automatic paste there.

Until the GNOME adapter is verified, use the GTK Start/Stop controls and manually
paste copied text. If clipboard delivery is unavailable, completed transcripts
remain accessible in enabled history; file transcription also prints to stdout.
The shortcut editor reports unsupported desktop integration rather than editing
Hyprland configuration on GNOME.

Before promoting Ubuntu support, test an actual supported Ubuntu GNOME/Wayland
session for microphone permission and default-device changes, GTK window and
background lifecycle, clipboard persistence after closing the window, focus
restoration, custom shortcut registration and release events, cancellation, and
package install/update/removal. Investigate GNOME's portal-supported global
shortcut facilities and explicit remote-desktop permission model for paste; do
not silently require broad input-injection privileges.

Release artifacts share the same Linux x86_64 payload. Distribution installers
supply appropriate dependencies; the user installer only activates Omarchy
integration when that desktop is present. A Debian package and GNOME-specific
adapter can be added without forking the speech engine or release repository.
