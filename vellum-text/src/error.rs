use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// 来自 PDF 层的错误，透明转发显示信息
    #[error(transparent)]
    Pdf(#[from] vellum_pdf::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
