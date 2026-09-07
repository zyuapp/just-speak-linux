-- Add this snippet to ~/.config/hypr/bindings.lua (Omarchy 4 / Hyprland 0.56).
-- F9 is Omarchy's voxtype push-to-talk key when voxtype is installed.
-- This deliberately replaces both existing F9 bindings with JustSpeak.
hl.unbind("F9")
o.bind("F9", "JustSpeak: start dictation", "just-speak start")
o.bind("F9", "JustSpeak: stop and paste", "just-speak stop", { release = true })

-- Escape still reaches the focused application. Cancel is a no-op when idle.
-- No compositor callback blocks waiting for the daemon or inference.
o.bind("ESCAPE", "JustSpeak: cancel dictation", "just-speak cancel", { non_consuming = true })
