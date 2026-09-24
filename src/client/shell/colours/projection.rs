//! Borrow the resolved base theme's tonal character without reallocating identity hues.
use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct ThemeTone {
    lightness: f64,
    chroma: f64,
}

impl ThemeTone {
    pub(super) fn from_palette(palette: &Palette) -> Option<Self> {
        let Color::Rgb(r, g, b) = palette.text else {
            // ANSI/terminal colours have no fixed RGB without a host response.
            return None;
        };
        let mut chromas = [
            palette.accent,
            palette.mauve,
            palette.green,
            palette.yellow,
            palette.red,
            palette.blue,
            palette.teal,
            palette.peach,
        ]
        .into_iter()
        .filter_map(|colour| match colour {
            Color::Rgb(r, g, b) => Some(Rgb([r, g, b]).oklch()[1]),
            _ => None,
        })
        .filter(|chroma| *chroma > 0.015)
        .collect::<Vec<_>>();
        if chromas.is_empty() {
            return None;
        }
        chromas.sort_by(f64::total_cmp);
        Some(Self {
            lightness: Rgb([r, g, b]).oklch()[0],
            chroma: chromas[chromas.len() / 2],
        })
    }
    pub(super) fn text(self, light: bool, tab: bool, detail: bool) -> (f64, f64) {
        let offset = if detail {
            0.18
        } else if tab {
            0.04
        } else {
            0.08
        };
        let lightness = if light {
            (self.lightness + offset * 0.4).clamp(0.25, 0.48)
        } else {
            (self.lightness - offset).clamp(0.64, 0.88)
        };
        let strength = if detail {
            if tab {
                0.42
            } else {
                0.20
            }
        } else if tab {
            0.65
        } else {
            0.48
        };
        (lightness, (self.chroma * strength).clamp(0.012, 0.095))
    }
}
