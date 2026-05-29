use serde_json::{json, Value};
use vellum_text::sink::{DebugEvent, DebugSink};

/// Records every pipeline event as a JSON array entry.
/// Call [`finish`] to consume the sink and get the full trace.
pub struct JsonTraceSink {
    events: Vec<Value>,
}

impl JsonTraceSink {
    pub fn new() -> Self {
        Self { events: vec![] }
    }

    pub fn finish(self) -> Value {
        json!({ "events": self.events })
    }
}

impl Default for JsonTraceSink {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugSink for JsonTraceSink {
    fn emit(&mut self, event: DebugEvent<'_>) {
        let entry = match event {
            DebugEvent::GlyphExtracted(g) => json!({
                "stage":  "glyph",
                "id":     g.glyph_id,
                "unicode": g.unicode.map(|c| c.to_string()),
                "method": format!("{:?}", g.mapping_method),
                "font":   g.font_name,
                "x":      g.origin.x,
                "y":      g.origin.y,
            }),
            DebugEvent::UnicodeResolved { glyph_id, result, method } => json!({
                "stage":    "mapping",
                "glyph_id": glyph_id,
                "unicode":  result.map(|c| c.to_string()),
                "method":   format!("{:?}", method),
            }),
            DebugEvent::WordFormed(w) => json!({
                "stage": "word",
                "text":  w.text,
                "x": w.bounds.min.x, "y": w.bounds.min.y,
                "w": w.bounds.width(), "h": w.bounds.height(),
            }),
            DebugEvent::LineFormed(l) => json!({
                "stage":    "line",
                "words":    l.words.len(),
                "baseline": l.baseline_y,
            }),
            DebugEvent::BlockFormed(b) => json!({
                "stage": "block",
                "lines": b.lines.len(),
                "order": b.reading_order,
            }),
            DebugEvent::ReadingOrderDetermined(order) => json!({
                "stage": "reading_order",
                "order": order,
            }),
        };
        self.events.push(entry);
    }
}
