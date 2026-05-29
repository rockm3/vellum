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
    pub fn map_glyphs(&self, glyphs: &mut [RawGlyph]) {
        for g in glyphs.iter_mut() {
            if let Some(cmap) = self.cmaps.get(&g.font_name) {
                if let Some(ch) = cmap.get(g.glyph_id) {
                    g.unicode         = Some(ch);
                    g.mapping_method  = MappingMethod::ToUnicode;
                }
            }
        }
    }

    /// 已成功映射的字形数量（用于调试统计）。
    pub fn mapped_count(&self, glyphs: &[RawGlyph]) -> usize {
        glyphs.iter().filter(|g| g.unicode.is_some()).count()
    }
}
