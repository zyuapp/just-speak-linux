-- Add this snippet to ~/.config/hypr/bindings.lua (Omarchy 4 / Hyprland 0.56).
-- JustSpeak uses F10; Omarchy's Voxtype F9 bindings remain available.
-- Check for custom F10 bindings before applying this snippet.
hl.unbind("F10")
o.bind("F10", "JustSpeak: start dictation", "just-speak start")
o.bind("F10", "JustSpeak: stop and paste", "just-speak stop", { release = true })

-- Escape still reaches the focused application. Cancel is a no-op when idle.
-- No compositor callback blocks waiting for the daemon or inference.
o.bind("ESCAPE", "JustSpeak: cancel dictation", "just-speak cancel", { non_consuming = true })
