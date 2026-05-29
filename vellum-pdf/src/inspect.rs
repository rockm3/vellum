use crate::xref::{ObjectId, Trailer, XRefEntry};

/// 解析过程中依次触发的事件，按发生顺序排列。
#[derive(Debug)]
pub enum ParseEvent<'a> {
    /// 找到 `startxref` 关键字，`offset` 为 XRef 节区的字节位置。
    StartXRef { offset: usize },
    /// 从 XRef 表中解码出一条交叉引用记录。
    XRefEntry { id: ObjectId, entry: &'a XRefEntry },
    /// trailer 字典已解码完毕。
    Trailer(&'a Trailer),
    /// 间接对象被从源字节中访问（懒加载触发点）。
    ObjectAccess { id: ObjectId, at_offset: usize },
}

/// 解析事件接收器。实现此 trait 可在解析过程中观测内部状态。
pub trait ParseSink {
    fn on_parse_event(&mut self, event: ParseEvent<'_>);
}

/// 空接收器，生产构建中零开销。
pub struct NullParseSink;

impl ParseSink for NullParseSink {
    #[inline(always)]
    fn on_parse_event(&mut self, _: ParseEvent<'_>) {}
}
