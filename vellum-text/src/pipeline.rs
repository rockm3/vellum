use vellum_pdf::Document;
use crate::Result;
use crate::{
    sink::{DebugSink, NullSink},
    TextBlock,
};

pub struct TextPipeline<S: DebugSink = NullSink> {
    sink: S,
}

impl TextPipeline {
    pub fn new() -> Self {
        Self { sink: NullSink }
    }
}

impl Default for TextPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: DebugSink> TextPipeline<S> {
    pub fn with_sink(sink: S) -> Self {
        Self { sink }
    }

    /// 对单页运行完整提取流水线。
    ///
    /// 阶段（逐步填充）：
    ///   Phase 2 ✓  内容流解码 → RawGlyph 列表
    ///   Phase 3 ✓  ToUnicode CMap → 填充 unicode 字段
    ///   Phase 4 ✓  字形聚类 → 词 → 行 → 块
    ///   Phase 5 ✓  XY-Cut 阅读顺序
    pub fn extract_page(&mut self, doc: &Document, page_index: u32) -> Result<Vec<TextBlock>> {
        // Phase 2
        let bytes  = doc.page_content_stream(page_index)?;
        let mut glyphs = crate::interpreter::StreamInterpreter::new().run(&bytes);

        // Phase 3
        let raw_cmaps = doc.page_to_unicode_cmaps(page_index)?;
        let mapper    = crate::unicode::UnicodeMapper::new(raw_cmaps);
        mapper.map_glyphs(&mut glyphs);

        // Phase 4
        let mut blocks = crate::cluster::cluster(glyphs);

        // Phase 5
        crate::order::assign_reading_order(&mut blocks);

        let _ = &mut self.sink;
        Ok(blocks)
    }
}
