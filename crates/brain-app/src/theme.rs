//! Colours and small formatting helpers shared by the views.

use gpui::{rgb, rgba, Rgba};

pub fn window_bg() -> Rgba { rgb(0x0b0d12) }
pub fn chrome_bg() -> Rgba { rgb(0x11141a) }
pub fn panel_bg() -> Rgba { rgb(0x0e1117) }
pub fn card_bg() -> Rgba { rgb(0x161a22) }
pub fn card_selected_bg() -> Rgba { rgb(0x1c1f2b) }
pub fn row_hover_bg() -> Rgba { rgb(0x151922) }
pub fn border() -> Rgba { rgb(0x1d212b) }
pub fn card_border() -> Rgba { rgb(0x242a36) }

pub fn text() -> Rgba { rgb(0xd7dae0) }
pub fn text_strong() -> Rgba { rgb(0xffffff) }
pub fn text_muted() -> Rgba { rgb(0x7c8394) }
pub fn text_faint() -> Rgba { rgb(0x5b6272) }

pub fn red() -> Rgba { rgb(0xff4d5e) }
pub fn red_soft() -> Rgba { rgb(0xff8a95) }
pub fn red_tint() -> Rgba { rgba(0xff4d5e1f) }
pub fn red_edge() -> Rgba { rgba(0xff4d5e88) }
pub fn amber() -> Rgba { rgb(0xf5b942) }
pub fn amber_edge() -> Rgba { rgba(0xf5b94255) }
pub fn blue() -> Rgba { rgb(0x4da3ff) }
pub fn green() -> Rgba { rgb(0x3ccf91) }
pub fn grey() -> Rgba { rgb(0x4a5060) }

/// Badge colours per account: main is blue, the next ones purple, teal, pink.
pub fn account_colors(index: usize) -> (Rgba, Rgba) {
    const PALETTE: [(u32, u32); 4] = [
        (0x4da3ff26, 0x8cc4ff),
        (0xb07cff26, 0xcdaaff),
        (0x2fd0c226, 0x7fe6dc),
        (0xff6fb526, 0xffa3d1),
    ];
    let (bg, fg) = PALETTE[index % PALETTE.len()];
    (rgba(bg), rgb(fg))
}

/// "gerade", "4 min", "2 h", "3 d"
pub fn ago(ms: i64, now_ms: i64) -> String {
    let secs = ((now_ms - ms) / 1000).max(0);
    match secs {
        0..=59 => "gerade".into(),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86_399 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86_400),
    }
}

/// Replaces the home directory with `~`.
pub fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}
