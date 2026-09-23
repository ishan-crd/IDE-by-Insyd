//! Embedded assets (icons). Everything ships inside the binary: no files to
//! locate at runtime, and only icons actually drawn get rasterized.

use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$( (concat!("icons/", $name, ".svg"), include_bytes!(concat!("../../../assets/icons/", $name, ".svg"))) ),*]
    };
}

static ICONS: &[(&str, &[u8])] = icons![
    "brain",
    "brain-links",
    "refresh",
    "play",
    "layout",
    "moon",
    "sun",
    "pr",
    "branch",
    "plus",
    "minus",
    "chevron-down",
    "chevron-right",
    "chevron-left",
    "handoff",
    "history",
    "settings",
    "popout",
    "search",
    "send",
    "stop",
    "check",
    "running",
    "cross",
    "close",
    "folder",
    "file",
    "trash",
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, b)) = ICONS.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(b)));
        }
        // Fall back to gpui-component's bundled icons (used by its widgets).
        gpui_component_assets(path)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}

fn gpui_component_assets(path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
    gpui_kit_assets::Assets.load(path).or(Ok(None))
}
