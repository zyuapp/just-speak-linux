//! Desktop capabilities are separate from speech, settings, and history.
//! GNOME/other Wayland sessions currently expose clipboard delivery only.
mod hyprland;
use anyhow::Result;
pub use hyprland::{PasteTarget, copy, paste_if_current};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Capabilities {
    pub name: &'static str,
    pub automatic_paste: bool,
    pub shortcut_editing: bool,
    pub experimental: bool,
}

pub fn capabilities() -> Capabilities {
    let hyprland = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some();
    Capabilities {
        name: if hyprland {
            "hyprland"
        } else {
            "wayland-clipboard"
        },
        automatic_paste: hyprland,
        shortcut_editing: hyprland,
        experimental: !hyprland,
    }
}

pub fn capture_target() -> Result<Option<PasteTarget>> {
    if capabilities().automatic_paste {
        hyprland::capture_target().map(Some)
    } else {
        Ok(None)
    }
}

pub fn check() -> Result<()> {
    if capabilities().automatic_paste {
        hyprland::check()
    } else {
        Ok(())
    }
}
