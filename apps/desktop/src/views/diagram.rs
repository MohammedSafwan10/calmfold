//! ```` ```mermaid ```` diagrams in notes.
//!
//! `mermaid-rs-renderer` (pure Rust, the renderer Zed's Markdown preview uses)
//! turns the source into SVG with colors taken from the app theme, and GPUI's
//! own SVG renderer rasterizes it at the window's scale. The editor shows the
//! picture when the caret is elsewhere and the source when it is inside.
//!
//! Results are cached by source; a theme or scale change clears the cache.
//! A diagram that doesn't parse stays as its source (no error box), so a
//! half-typed diagram never flickers.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::{Hsla, RenderImage, Rgba, SvgRenderer};

/// Diagram sources longer than this aren't rendered.
const MAX_SOURCE_BYTES: usize = 32 * 1024;
const MAX_CACHED: usize = 128;

type Rendered = Option<(Arc<RenderImage>, f32, f32)>;

/// Colors from the theme, as `#rrggbb` (what the renderer expects).
#[derive(Clone, PartialEq)]
pub struct DiagramColors {
    pub fill: String,
    pub fill_alt: String,
    pub text: String,
    pub border: String,
    pub line: String,
    pub background: String,
    pub accent: String,
    pub font_family: String,
    pub font_size: f32,
}

impl DiagramColors {
    pub fn from_theme(theme: &gpui_kit::component::Theme) -> Self {
        Self {
            fill: hex(theme.secondary),
            fill_alt: hex(theme.muted),
            text: hex(theme.foreground),
            border: hex(theme.border),
            line: hex(theme.muted_foreground),
            background: hex(theme.background),
            accent: hex(theme.blue),
            font_family: theme.font_family.to_string(),
            font_size: f32::from(theme.font_size) * 0.9,
        }
    }
}

pub struct Diagrams {
    svg: SvgRenderer,
    colors: RefCell<DiagramColors>,
    scale: RefCell<f32>,
    cache: RefCell<HashMap<String, Rendered>>,
}

impl Diagrams {
    pub fn new(svg: SvgRenderer, colors: DiagramColors, scale: f32) -> Rc<Self> {
        Rc::new(Self {
            svg,
            colors: RefCell::new(colors),
            scale: RefCell::new(scale),
            cache: RefCell::new(HashMap::new()),
        })
    }

    /// New theme colors or display scale: re-render on next paint.
    pub fn update(&self, colors: DiagramColors, scale: f32) {
        let changed = *self.colors.borrow() != colors || *self.scale.borrow() != scale;
        if changed {
            *self.colors.borrow_mut() = colors;
            *self.scale.borrow_mut() = scale;
            self.cache.borrow_mut().clear();
        }
    }

    /// The rendered diagram and its logical size, or `None` to show the source.
    pub fn render(&self, source: &str) -> Rendered {
        if source.trim().is_empty() || source.len() > MAX_SOURCE_BYTES {
            return None;
        }
        if let Some(hit) = self.cache.borrow().get(source) {
            return hit.clone();
        }
        let result = self.render_uncached(source);
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= MAX_CACHED {
            cache.clear();
        }
        cache.insert(source.to_owned(), result.clone());
        result
    }

    fn render_uncached(&self, source: &str) -> Rendered {
        let svg = svg_for(source, &self.colors.borrow())?;
        let scale = *self.scale.borrow();
        let image = self.svg.render_single_frame(svg.as_bytes(), scale).ok()?;
        // The logical size is the SVG's own; the raster's pixel size depends on
        // how GPUI's renderer oversamples, so it can't be divided back out.
        let (w, h) = svg_size(&svg)?;
        (w > 0. && h > 0.).then_some((image, w, h))
    }
}

/// Mermaid → themed SVG. `None` if the source doesn't parse.
pub fn svg_for(source: &str, colors: &DiagramColors) -> Option<String> {
    let mut theme = mermaid_rs_renderer::Theme::modern();
    theme.font_family = format!("{}, -apple-system, sans-serif", colors.font_family);
    theme.font_size = colors.font_size;
    theme.primary_color = colors.fill.clone();
    theme.secondary_color = colors.fill_alt.clone();
    theme.tertiary_color = colors.fill.clone();
    theme.primary_text_color = colors.text.clone();
    theme.text_color = colors.text.clone();
    theme.primary_border_color = colors.border.clone();
    theme.line_color = colors.line.clone();
    theme.edge_label_background = colors.background.clone();
    theme.cluster_background = colors.fill_alt.clone();
    theme.cluster_border = colors.border.clone();
    // Transparent: the diagram sits on the note like text does.
    theme.background = "transparent".into();
    theme.sequence_actor_fill = colors.fill.clone();
    theme.sequence_actor_border = colors.border.clone();
    theme.sequence_actor_line = colors.line.clone();
    theme.sequence_note_fill = colors.fill_alt.clone();
    theme.sequence_note_border = colors.accent.clone();
    theme.sequence_activation_fill = colors.fill_alt.clone();
    theme.sequence_activation_border = colors.border.clone();
    theme.pie_title_text_color = colors.text.clone();
    theme.pie_section_text_color = colors.text.clone();
    theme.pie_legend_text_color = colors.text.clone();
    theme.pie_stroke_color = colors.background.clone();
    theme.pie_outer_stroke_color = colors.border.clone();
    let options = mermaid_rs_renderer::RenderOptions {
        theme,
        layout: mermaid_rs_renderer::LayoutConfig::default(),
    };
    // A panic in third-party layout code must never take the note down.
    std::panic::catch_unwind(|| mermaid_rs_renderer::render_with_options(source, options).ok())
        .ok()
        .flatten()
}

/// `width` / `height` of the root `<svg>` element, in SVG user units (= logical px).
fn svg_size(svg: &str) -> Option<(f32, f32)> {
    let head = &svg[..svg.find('>')?];
    let attr = |name: &str| -> Option<f32> {
        let key = format!(" {name}=\"");
        let at = head.find(&key)? + key.len();
        head[at..]
            .split('"')
            .next()?
            .trim_end_matches("px")
            .parse()
            .ok()
    };
    Some((attr("width")?, attr("height")?))
}

fn hex(c: Hsla) -> String {
    let c: Rgba = c.into();
    let b = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors() -> DiagramColors {
        DiagramColors {
            fill: "#1c1c1c".into(),
            fill_alt: "#171717".into(),
            text: "#fafafa".into(),
            border: "#232323".into(),
            line: "#8f8f8f".into(),
            background: "#0a0a0a".into(),
            accent: "#60a5fa".into(),
            font_family: "Inter".into(),
            font_size: 14.,
        }
    }

    #[test]
    fn renders_common_diagrams_with_theme_colors() {
        for src in [
            "graph TD\n  A[Start] --> B{Ok?}\n  B -->|yes| C[Done]",
            "sequenceDiagram\n  Alice->>Bob: Hi\n  Bob-->>Alice: Hello",
            "pie title Pets\n  \"Dogs\" : 3\n  \"Cats\" : 2",
        ] {
            let svg = svg_for(src, &colors()).unwrap_or_else(|| panic!("renders: {src}"));
            assert!(svg.contains("<svg"), "{src}");
            assert!(
                svg.to_lowercase().contains("#fafafa"),
                "uses the theme's text color"
            );
        }
    }

    #[test]
    fn broken_source_stays_as_text() {
        assert!(svg_for("this is not mermaid", &colors()).is_none());
    }

    #[test]
    fn size_comes_from_the_svg_element() {
        let svg =
            "<svg xmlns=\"x\" width=\"450\" height=\"265.5\" viewBox=\"0 0 450 265\"><rect/></svg>";
        assert_eq!(svg_size(svg), Some((450., 265.5)));
        assert_eq!(svg_size("<svg viewBox=\"0 0 1 1\">"), None);
    }

    #[test]
    fn hex_is_six_digits() {
        let c = gpui_kit::hsla(0., 0., 1., 1.);
        assert_eq!(hex(c), "#ffffff");
    }
}
