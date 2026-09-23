//! Turns settings into app behavior: theme, accent, approval policy, defaults.

use gpui::{App, WindowAppearance};
use insyde_core::agents::acp::Policy;
use insyde_core::agents::{AgentId, AgentSpec, DEFAULT_AGENT};
use insyde_core::settings::{self, Approval, ThemeChoice};
use insyde_theme::Mode;

/// Install the theme and accent from settings (following the OS when "System").
pub fn apply_theme(cx: &mut App) {
    let s = settings::get();
    let mode = match s.theme {
        ThemeChoice::Dark => Mode::Dark,
        ThemeChoice::Light => Mode::Light,
        ThemeChoice::System => match cx.window_appearance() {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Mode::Light,
            _ => Mode::Dark,
        },
    };
    let accent_name = serde_json::to_value(s.accent)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let accent = insyde_theme::accent_hex(&accent_name).map(insyde_theme::color);
    insyde_theme::init_with_accent(cx, mode, accent);
    crate::sync_component_theme(cx);
    cx.refresh_windows();
}

pub fn policy() -> Policy {
    match settings::get().approval {
        Approval::Ask => Policy::Ask,
        Approval::AcceptEdits => Policy::AcceptEdits,
        Approval::FullAccess => Policy::FullAccess,
    }
}

/// The agent used by "+ Agent", ⌘T and new worktrees.
pub fn default_agent() -> AgentId {
    let key = settings::get().default_agent;
    AgentSpec::by_key(&key)
        .filter(|s| s.acp.is_some())
        .map(|s| s.id)
        .unwrap_or(DEFAULT_AGENT)
}

pub fn reduce_motion() -> bool {
    settings::get().reduce_motion
}
