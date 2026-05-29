use std::collections::HashMap;
use crate::cmap::CMap;
use crate::glyph::{MappingMethod, RawGlyph};

/// Unicode 映射器：将 `RawGlyph` 列表中 `Unmapped` 的字形按 ToUnicode CMap 填充 unicode 字段。
pub struct UnicodeMapper {
    cmaps: HashMap<String, CMap>,
}

impl UnicodeMapper {
    /// 从原始 CMap 字节构建映射器（key 同 `Document::page_to_unicode_cmaps` 的格式）。
    pub fn new(raw: HashMap<String, Vec<u8>>) -> Self {
        let cmaps = raw.into_iter()
            .map(|(k, v)| (k, CMap::parse(&v)))
            .collect();
        Self { cmaps }
    }

    /// 对字形列表原地填充 `unicode` 与 `mapping_method`。
    ///
    /// 映射优先级：
    ///   1. ToUnicode CMap（最可靠）
    ///   2. 标准编码回退：0x20-0x7E = ASCII，0xA0-0xFF = Latin-1
    ///      覆盖 WinAnsiEncoding / MacRomanEncoding / StandardEncoding 绝大多数拉丁字符。
    ///      0x80-0x9F 是 Windows-1252 私有区，此处略过。
    pub fn map_glyphs(&self, glyphs: &mut [RawGlyph]) {
        for g in glyphs.iter_mut() {
            // ── 优先：ToUnicode CMap ───────────────────────────────────────
            if let Some(cmap) = self.cmaps.get(&g.font_name) {
                if let Some(ch) = cmap.get(g.glyph_id) {
                    g.unicode        = Some(ch);
                    g.mapping_method = MappingMethod::ToUnicode;
                    continue;
                }
            }
            // ── 回退：标准编码字体（无 ToUnicode 的 14 标准字体等）────────
            let id = g.glyph_id;
            if (0x20..=0x7E).contains(&id) || (0xA0..=0xFF).contains(&id) {
                if let Some(ch) = char::from_u32(id) {
                    g.unicode        = Some(ch);
                    g.mapping_method = MappingMethod::Heuristic;
                }
            }
        }
    }

    /// 已成功映射的字形数量（用于调试统计）。
    pub fn mapped_count(&self, glyphs: &[RawGlyph]) -> usize {
        glyphs.iter().filter(|g| g.unicode.is_some()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vellum_pdf::{Point, Rect};

    fn glyph(id: u32, font: &str) -> RawGlyph {
        RawGlyph {
            glyph_id: id,
            unicode: None,
            mapping_method: MappingMethod::Unmapped,
            origin: Point { x: 0.0, y: 0.0 },
            bounds: Rect::new(0.0, 0.0, 6.0, 12.0),
            font_size: 12.0,
            font_name: font.to_string(),
        }
    }

    fn mapper_empty() -> UnicodeMapper {
        UnicodeMapper::new(HashMap::new())
    }

    #[test]
    fn ascii_range_falls_back_to_heuristic() {
        let mut g = vec![glyph(b'A' as u32, "/F1"), glyph(b'z' as u32, "/F1")];
        mapper_empty().map_glyphs(&mut g);
        assert_eq!(g[0].unicode, Some('A'));
        assert_eq!(g[0].mapping_method, MappingMethod::Heuristic);
        assert_eq!(g[1].unicode, Some('z'));
    }

    #[test]
    fn latin1_supplement_falls_back() {
        // 0xE9 = é (Latin-1)
        let mut g = vec![glyph(0xE9, "/F1")];
        mapper_empty().map_glyphs(&mut g);
        assert_eq!(g[0].unicode, Some('é'));
        assert_eq!(g[0].mapping_method, MappingMethod::Heuristic);
    }

    #[test]
    fn control_range_0x80_0x9f_not_mapped() {
        // Windows-1252 私有区：不做启发映射
        let mut g = vec![glyph(0x80, "/F1"), glyph(0x9F, "/F1")];
        mapper_empty().map_glyphs(&mut g);
        assert!(g[0].unicode.is_none());
        assert!(g[1].unicode.is_none());
    }

    #[test]
    fn tounicode_cmap_takes_priority_over_heuristic() {
        // CMap 中 0x41 → '模'，应优先于 ASCII 回退的 'A'
        let raw = HashMap::from([(
            "/F1".to_string(),
            b"beginbfchar\n<41> <6A21>\nendbfchar\n".to_vec(),
        )]);
        let mapper = UnicodeMapper::new(raw);
        let mut g = vec![glyph(0x41, "/F1")];
        mapper.map_glyphs(&mut g);
        assert_eq!(g[0].unicode, Some('模'));
        assert_eq!(g[0].mapping_method, MappingMethod::ToUnicode);
    }

    #[test]
    fn null_byte_not_mapped() {
        let mut g = vec![glyph(0x00, "/F1"), glyph(0x1F, "/F1")];
        mapper_empty().map_glyphs(&mut g);
        assert!(g[0].unicode.is_none());
        assert!(g[1].unicode.is_none());
    }
}
