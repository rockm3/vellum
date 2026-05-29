use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("PDF parse error: {0}")]
    Lopdf(#[from] lopdf::Error),

    /// 请求的页面索引超出文档范围
    #[error("page {index} not found")]
    PageNotFound { index: u32 },

    /// 字节流不符合 PDF 规范
    #[error("malformed content stream")]
    MalformedContent,

    /// 遇到本引擎尚未支持的 PDF 特性
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(&'static str),
}
