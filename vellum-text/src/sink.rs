use crate::{Line, MappingMethod, RawGlyph, TextBlock, Word};

/// 文本流水线各阶段触发的调试事件。
pub enum DebugEvent<'a> {
    GlyphExtracted(&'a RawGlyph),
    UnicodeResolved {
        glyph_id: u32,
        result: Option<char>,
        method: MappingMethod,
    },
    WordFormed(&'a Word),
    LineFormed(&'a Line),
    BlockFormed(&'a TextBlock),
    /// 阅读顺序已确定，参数为各文本块的排列索引序列。
    ReadingOrderDetermined(&'a [usize]),
}

/// 调试事件接收器 trait，实现此 trait 可观测流水线内部状态。
pub trait DebugSink {
    fn emit(&mut self, event: DebugEvent<'_>);
}

/// 空接收器，生产构建中所有调用均被内联消除。
pub struct NullSink;

impl DebugSink for NullSink {
    #[inline(always)]
    fn emit(&mut self, _: DebugEvent<'_>) {}
}
