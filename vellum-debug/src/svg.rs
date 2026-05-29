use std::fmt::Write as _;
use vellum_text::sink::{DebugEvent, DebugSink};

/// Accumulates SVG geometry for each pipeline stage.
/// Call [`render`] when done to get a single SVG string with layered groups
/// that can be overlaid on a page rendering.
pub struct SvgOverlaySink {
    page_width: f32,
    page_height: f32,
    layers: Layers,
}

#[derive(Default)]
struct Layers {
    glyphs:  String,
    words:   String,
    lines:   String,
    blocks:  String,
    order:   String,
}

impl SvgOverlaySink {
    pub fn new(page_width: f32, page_height: f32) -> Self {
        Self { page_width, page_height, layers: Layers::default() }
    }

    pub fn render(&self) -> String {
        // PDF y-axis points up; SVG points down.  A `transform="scale(1,-1)"` on the
        // outer group flips the coordinate system so overlays align with a normal SVG
        // page render that has already been flipped.
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
  <g transform="translate(0,{h}) scale(1,-1)">
    <g id="glyphs" opacity="0.35">{glyphs}</g>
    <g id="words"  opacity="0.45">{words}</g>
    <g id="lines"  opacity="0.50">{lines}</g>
    <g id="blocks" opacity="0.60">{blocks}</g>
    <g id="order">{order}</g>
  </g>
</svg>"#,
            w = self.page_width,
            h = self.page_height,
            glyphs = self.layers.glyphs,
            words  = self.layers.words,
            lines  = self.layers.lines,
            blocks = self.layers.blocks,
            order  = self.layers.order,
        )
    }

    fn push_rect(buf: &mut String, x: f32, y: f32, w: f32, h: f32, color: &str) {
        let _ = write!(
            buf,
            r#"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" fill="none" stroke="{color}" stroke-width="0.5"/>"#
        );
    }
}

impl DebugSink for SvgOverlaySink {
    fn emit(&mut self, event: DebugEvent<'_>) {
        match event {
            DebugEvent::GlyphExtracted(g) => {
                let r = &g.bounds;
                Self::push_rect(&mut self.layers.glyphs, r.min.x, r.min.y, r.width(), r.height(), "#4488ff");
            }
            DebugEvent::WordFormed(w) => {
                let r = &w.bounds;
                Self::push_rect(&mut self.layers.words, r.min.x, r.min.y, r.width(), r.height(), "#44bb44");
            }
            DebugEvent::LineFormed(l) => {
                let r = &l.bounds;
                Self::push_rect(&mut self.layers.lines, r.min.x, r.min.y, r.width(), r.height(), "#ff8800");
            }
            DebugEvent::BlockFormed(b) => {
                let r = &b.bounds;
                Self::push_rect(&mut self.layers.blocks, r.min.x, r.min.y, r.width(), r.height(), "#cc0000");
            }
            DebugEvent::ReadingOrderDetermined(order) => {
                // TODO: draw numbered arrows between block centroids
                let _ = write!(self.layers.order, "<!-- order: {order:?} -->");
            }
            _ => {}
        }
    }
}
