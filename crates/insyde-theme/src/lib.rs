//! Design tokens for InsyDE, transcribed 1:1 from `design/InsyDE.dc.html`
//! (`.theme-light` / `.theme-dark`). Views must read colors, radii and type
//! sizes from here; no hex literals in the UI crate outside this file.

use gpui::{App, BoxShadow, Global, Hsla, Pixels, point, px, rgba};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

/// Semantic color tokens. Names match the design's CSS variables.
#[derive(Clone, Debug)]
pub struct Theme {
    pub mode: Mode,
    pub ground: Hsla,
    pub panel: Hsla,
    pub panel_2: Hsla,
    pub line: Hsla,
    pub line_soft: Hsla,
    pub field_border: Hsla,
    pub hover: Hsla,
    pub hover_2: Hsla,
    pub ink: Hsla,
    pub ink_2: Hsla,
    pub ink_3: Hsla,
    pub ink_4: Hsla,
    pub ink_disabled: Hsla,
    pub ink_faint: Hsla,
    pub primary: Hsla,
    pub primary_hover: Hsla,
    pub on_primary: Hsla,
    pub accent: Hsla,
    pub sel_bg: Hsla,
    pub sel_chip: Hsla,
    pub sel_text: Hsla,
    pub seg_active: Hsla,
    pub ok: Hsla,
    pub err: Hsla,
    pub warn: Hsla,
    pub err_bg: Hsla,
    pub err_border: Hsla,
    pub scroll_thumb: Hsla,
    /// Window backdrop outside the app (`html,body` background).
    pub backdrop: Hsla,
    /// Glass mode: the window chrome is translucent over a blurred desktop
    /// and the working surfaces float on it as solid sheets.
    pub glass: bool,
    /// Background of the window chrome (top bar, sidebars, status bar).
    /// Equal to `panel` unless glass is on.
    pub chrome: Hsla,
    /// Window ground behind everything. Equal to `ground` unless glass is on.
    pub chrome_ground: Hsla,
    /// Dividers drawn on the chrome.
    pub chrome_line: Hsla,
    /// Fixed brand/data colors that do not change with the theme.
    pub palette: Palette,
}

/// Fixed accent colors used for agent monograms, brain node types and
/// terminal dots. Identical in both themes, as in the design.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub blue: Hsla,
    pub teal: Hsla,
    pub green: Hsla,
    pub red: Hsla,
    pub amber: Hsla,
    pub purple: Hsla,
    pub orange: Hsla,
    pub gray: Hsla,
    pub graphite: Hsla,
    pub slate: Hsla,
    pub stone: Hsla,
    pub white: Hsla,
    pub traffic_red: Hsla,
    pub traffic_yellow: Hsla,
    pub traffic_green: Hsla,
}

fn hex(v: u32) -> Hsla {
    gpui::rgb(v).into()
}

impl Palette {
    fn new() -> Self {
        Self {
            blue: hex(0x3B7DD8),
            teal: hex(0x2A9D9F),
            green: hex(0x2E9E6B),
            red: hex(0xD95F6E),
            amber: hex(0xD8A23A),
            purple: hex(0x7C5CD6),
            orange: hex(0xE0733A),
            gray: hex(0x8A8A84),
            graphite: hex(0x3D3D3A),
            slate: hex(0x5E5E58),
            stone: hex(0x706F6A),
            white: hex(0xFFFFFF),
            traffic_red: hex(0xFF5F57),
            traffic_yellow: hex(0xFEBC2E),
            traffic_green: hex(0x28C840),
        }
    }
}

impl Theme {
    pub fn light() -> Self {
        Self {
            mode: Mode::Light,
            ground: hex(0xEDEDEB),
            panel: hex(0xFFFFFF),
            panel_2: hex(0xFAFAF9),
            line: hex(0xE4E4E1),
            line_soft: hex(0xEFEFED),
            field_border: hex(0xDCDCD8),
            hover: hex(0xF3F3F1),
            hover_2: hex(0xF0F0EE),
            ink: hex(0x1A1A19),
            ink_2: hex(0x3D3D3A),
            ink_3: hex(0x706F6A),
            ink_4: hex(0x767671),
            ink_disabled: hex(0xB4B4AF),
            ink_faint: hex(0xA3A39E),
            primary: hex(0x1A1A19),
            primary_hover: hex(0x2E2E2C),
            on_primary: hex(0xFFFFFF),
            accent: hex(0x2F6BEB),
            sel_bg: hex(0xEDF2FE),
            sel_chip: hex(0xD6E2FD),
            sel_text: hex(0x1D49B8),
            seg_active: hex(0xFFFFFF),
            ok: hex(0x2E9E6B),
            err: hex(0xB3261E),
            warn: hex(0xD8A23A),
            err_bg: hex(0xFDECEC),
            err_border: hex(0xF0B4B7),
            scroll_thumb: hex(0xC9C9C4),
            backdrop: hex(0x0E0E0D),
            glass: false,
            chrome: hex(0xFFFFFF),
            chrome_ground: hex(0xEDEDEB),
            chrome_line: hex(0xE4E4E1),
            palette: Palette::new(),
        }
    }

    pub fn dark() -> Self {
        Self {
            mode: Mode::Dark,
            ground: hex(0x151514),
            panel: hex(0x1D1D1C),
            panel_2: hex(0x222221),
            line: hex(0x2D2D2B),
            line_soft: hex(0x282826),
            field_border: hex(0x383836),
            hover: hex(0x2A2A28),
            hover_2: hex(0x2F2F2D),
            ink: hex(0xF2F2F0),
            ink_2: hex(0xC9C9C5),
            ink_3: hex(0x9C9C97),
            ink_4: hex(0x8A8A84),
            ink_disabled: hex(0x55554F),
            ink_faint: hex(0x5E5E58),
            primary: hex(0xF2F2F0),
            primary_hover: hex(0xFFFFFF),
            on_primary: hex(0x1A1A19),
            accent: hex(0x3D74E8),
            sel_bg: hex(0x22304D),
            sel_chip: hex(0x2C3F6B),
            sel_text: hex(0xA9C2FF),
            seg_active: hex(0x3A3A38),
            ok: hex(0x3ECF8E),
            err: hex(0xF0A0A4),
            warn: hex(0xF2C14E),
            err_bg: hex(0x3A1F20),
            err_border: hex(0x6B3236),
            scroll_thumb: hex(0x4A4A47),
            backdrop: hex(0x0E0E0D),
            glass: false,
            chrome: hex(0x1D1D1C),
            chrome_ground: hex(0x151514),
            chrome_line: hex(0x2D2D2B),
            palette: Palette::new(),
        }
    }

    /// Turn on glass: chrome becomes translucent with the given tint
    /// (0 = clear, 1 = solid). Lines turn into faint light/dark hairlines
    /// so they read on any wallpaper.
    pub fn set_glass(&mut self, tint: f32) {
        let tint = tint.clamp(0.2, 0.95);
        self.glass = true;
        // One tinted layer only: the ground. Chrome regions sit on it without
        // their own fill (a faint lift in dark mode), otherwise the layers
        // stack up and the glass turns opaque.
        self.chrome_ground = self.ground.opacity(tint);
        self.chrome = if self.is_dark() {
            gpui::white().opacity(0.025)
        } else {
            gpui::white().opacity(0.18)
        };
        // Muted text sits on a busy, blurred backdrop: give it more contrast.
        if self.is_dark() {
            self.ink_3 = hex(0xB2B2AD);
            self.ink_4 = hex(0xA2A29C);
            self.ink_faint = hex(0x8A8A84);
        } else {
            self.ink_3 = hex(0x5A5A55);
            self.ink_4 = hex(0x62625D);
            self.ink_faint = hex(0x7E7E78);
        }
        self.chrome_line = if self.is_dark() {
            gpui::white().opacity(0.07)
        } else {
            gpui::black().opacity(0.07)
        };
    }

    /// Background for a surface nested inside a glass sheet: transparent in
    /// glass mode (so the sheet's rounded corners are not painted over),
    /// the given color otherwise.
    pub fn inner(&self, c: Hsla) -> Hsla {
        if self.glass {
            gpui::transparent_black()
        } else {
            c
        }
    }

    /// Recolor the accent (buttons, focus, selection, links) from one hue.
    /// Selection tints are derived so they stay legible in both modes.
    pub fn set_accent(&mut self, accent: Hsla) {
        let dark = self.is_dark();
        self.accent = if dark {
            Hsla {
                l: (accent.l + 0.06).min(0.72),
                ..accent
            }
        } else {
            accent
        };
        self.sel_bg = if dark {
            Hsla {
                l: 0.22,
                s: accent.s * 0.45,
                a: 1.,
                ..accent
            }
        } else {
            Hsla {
                l: 0.96,
                s: accent.s * 0.9,
                a: 1.,
                ..accent
            }
        };
        self.sel_chip = if dark {
            Hsla {
                l: 0.3,
                s: accent.s * 0.5,
                a: 1.,
                ..accent
            }
        } else {
            Hsla {
                l: 0.91,
                s: accent.s * 0.85,
                a: 1.,
                ..accent
            }
        };
        self.sel_text = if dark {
            Hsla {
                l: 0.83,
                s: accent.s * 0.9,
                a: 1.,
                ..accent
            }
        } else {
            Hsla {
                l: 0.42,
                a: 1.,
                ..accent
            }
        };
    }

    pub fn is_dark(&self) -> bool {
        self.mode == Mode::Dark
    }

    /// `--seg-shadow`: the raised look of the active segment in a segmented control.
    pub fn seg_shadow(&self) -> Vec<BoxShadow> {
        let (a, b) = if self.is_dark() {
            (rgba(0x00000066), rgba(0xffffff0f))
        } else {
            (rgba(0x1414131a), rgba(0x1414130f))
        };
        vec![
            BoxShadow {
                color: a.into(),
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(0.),
                inset: false,
            },
            BoxShadow {
                color: b.into(),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            },
        ]
    }

    /// `--shadow-pop` plus the larger drop used by menus and popovers.
    pub fn pop_shadow(&self, menu: bool) -> Vec<BoxShadow> {
        let a = if self.is_dark() {
            rgba(0x00000099)
        } else {
            rgba(0x1414132e)
        };
        let mut v = vec![BoxShadow {
            color: a.into(),
            offset: point(px(0.), px(4.)),
            blur_radius: px(12.),
            spread_radius: px(-4.),
            inset: false,
        }];
        if menu {
            v.push(BoxShadow {
                color: rgba(0x00000059).into(),
                offset: point(px(0.), px(12.)),
                blur_radius: px(32.),
                spread_radius: px(-12.),
                inset: false,
            });
        }
        v
    }
}

impl Global for Theme {}

/// Type scale and fixed metrics from the design (px at 1x).
pub mod metrics {
    use super::*;
    pub const TEXT: Pixels = px(13.);
    pub const TEXT_SM: Pixels = px(12.);
    pub const TEXT_XS: Pixels = px(11.);
    pub const TEXT_MONO: Pixels = px(11.5);
    pub const TEXT_TITLE: Pixels = px(14.);
    pub const TOPBAR_H: Pixels = px(48.);
    pub const STATUS_H: Pixels = px(26.);
    pub const TAB_H: Pixels = px(40.);
    pub const CONTROL_H: Pixels = px(30.);
    pub const RADIUS: Pixels = px(6.);
    pub const RADIUS_LG: Pixels = px(10.);
    pub const SIDE_W: f32 = 248.;
    pub const RIGHT_W: f32 = 380.;
    pub const TERM_H: f32 = 232.;
    pub const UI_FONT: &str = ".SystemUIFont";
    pub const MONO_FONT: &str = "Menlo";
    /// Serif italic for the "by Insyd" half of the wordmark (bundled, OFL).
    pub const BRAND_SERIF: &str = "Instrument Serif";
}

pub fn init(cx: &mut App, mode: Mode) {
    init_with_accent(cx, mode, None);
}

/// Install the theme for `mode`, optionally with a custom accent color.
pub fn init_with_accent(cx: &mut App, mode: Mode, accent: Option<Hsla>) {
    init_full(cx, mode, accent, None);
}

/// Install the theme with an optional accent and optional glass tint.
pub fn init_full(cx: &mut App, mode: Mode, accent: Option<Hsla>, glass: Option<f32>) {
    let mut t = match mode {
        Mode::Light => Theme::light(),
        Mode::Dark => Theme::dark(),
    };
    if let Some(a) = accent {
        t.set_accent(a);
    }
    if let Some(g) = glass {
        t.set_glass(g);
    }
    cx.set_global(t);
}

/// Built-in accent choices (hex), matching the design's palette.
pub fn accent_hex(name: &str) -> Option<u32> {
    match name {
        "violet" => Some(0x7C5CD6),
        "green" => Some(0x2E9E6B),
        "orange" => Some(0xE0733A),
        "pink" => Some(0xD6457F),
        _ => None, // blue = the design's own accent tokens
    }
}

pub fn color(hex_value: u32) -> Hsla {
    hex(hex_value)
}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}
