//! A QR code drawn with GPUI quads (ADR 0013). `qrcode` encodes the text
//! into modules; this paints the dark ones on a light tile, row by row, with
//! runs merged into one quad each. The tile is light in both themes so phone
//! cameras read it: the lighter of the theme's background and foreground is
//! the paper, the darker one the ink. Module edges snap to whole pixels.

use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, Bounds, Hsla, IntoElement, ParentElement as _, Rems, Styled as _, canvas, div, fill,
    point, px, size,
};
use qrcode::{Color, EcLevel, QrCode};

/// The light margin a scanner needs around the code, in modules (ISO/IEC 18004).
pub const QUIET_ZONE: usize = 4;

/// The encoded modules of one QR code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrMatrix {
    width: usize,
    dark: Vec<bool>,
}

impl QrMatrix {
    /// `None` only if `text` is too long for any QR version.
    #[must_use]
    pub fn encode(text: &str) -> Option<Self> {
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
        let width = code.width();
        let dark = code
            .to_colors()
            .into_iter()
            .map(|c| c == Color::Dark)
            .collect();
        Some(Self { width, dark })
    }

    /// Modules per side, without the quiet zone.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    #[must_use]
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.width && self.dark[y * self.width + x]
    }

    /// Runs of dark modules per row as `(row, first column, length)`.
    #[must_use]
    pub fn runs(&self) -> Vec<(usize, usize, usize)> {
        let mut runs = Vec::new();
        for y in 0..self.width {
            let mut x = 0;
            while x < self.width {
                if self.is_dark(x, y) {
                    let start = x;
                    while x < self.width && self.is_dark(x, y) {
                        x += 1;
                    }
                    runs.push((y, start, x - start));
                } else {
                    x += 1;
                }
            }
        }
        runs
    }
}

/// (paper, ink) for this theme: light tile, dark modules, in either theme.
fn colors(cx: &App) -> (Hsla, Hsla) {
    let theme = cx.theme();
    if theme.background.l >= theme.foreground.l {
        (theme.background, theme.foreground)
    } else {
        (theme.foreground, theme.background)
    }
}

/// The code on its light tile, `side` wide and tall.
pub fn qr_tile(matrix: Rc<QrMatrix>, side: Rems, cx: &App) -> impl IntoElement {
    let (paper, ink) = colors(cx);
    let runs = Rc::new(matrix.runs());
    let modules = matrix.width() + 2 * QUIET_ZONE;
    div()
        .size(side)
        .flex_shrink_0()
        .rounded(cx.theme().radius)
        // A hairline keeps the tile's edge visible on a light dialog.
        .border_1()
        .border_color(cx.theme().border)
        .bg(paper)
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    let side = bounds.size.width.min(bounds.size.height);
                    #[allow(clippy::cast_precision_loss)] // a few dozen modules
                    let total = modules as f32;
                    // Whole-pixel modules keep edges sharp; leftover space joins the margin.
                    let module = (side.as_f32() / total).floor().max(1.0);
                    let inset = ((side.as_f32() - module * total) / 2.0).floor();
                    #[allow(clippy::cast_precision_loss)]
                    let at = |n: usize| (n + QUIET_ZONE) as f32 * module;
                    for &(row, column, length) in runs.iter() {
                        #[allow(clippy::cast_precision_loss)]
                        let run = length as f32 * module;
                        let origin = point(
                            bounds.origin.x + px(inset + at(column)),
                            bounds.origin.y + px(inset + at(row)),
                        );
                        window
                            .paint_quad(fill(Bounds::new(origin, size(px(run), px(module))), ink));
                    }
                },
            )
            .size_full(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_setup_link_with_finder_patterns_and_merged_runs() {
        let uri = "otpauth://totp/Ottero:sam%40example.com?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZA&issuer=Ottero&digits=6&period=30";
        let m = QrMatrix::encode(uri).expect("fits");
        assert_eq!((m.width() - 17) % 4, 0, "a real QR version");
        assert!(m.width() >= 41, "long enough for version 6+");
        // Finder pattern corners: dark ring, light ring, dark centre.
        for (x, y) in [(0, 0), (m.width() - 7, 0), (0, m.width() - 7)] {
            assert!(m.is_dark(x, y) && m.is_dark(x + 6, y + 6));
            assert!(!m.is_dark(x + 1, y + 1) && m.is_dark(x + 3, y + 3));
        }
        let runs = m.runs();
        let painted: usize = runs.iter().map(|r| r.2).sum();
        let dark = (0..m.width())
            .flat_map(|y| (0..m.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| m.is_dark(x, y))
            .count();
        assert_eq!(painted, dark, "every dark module painted once");
        assert!(runs.len() < dark, "neighbours share a quad");
        assert!(!m.is_dark(m.width(), 0), "outside is light");
    }

    #[test]
    fn too_long_text_has_no_code() {
        assert!(QrMatrix::encode(&"x".repeat(5000)).is_none());
    }
}
