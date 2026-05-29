mod document;
mod error;
mod font;
pub mod inspect;
pub mod source;
mod types;
pub mod xref;

pub use document::Document;
pub use error::Error;
pub use font::{Font, PageFonts};
pub use types::{Matrix, Point, Rect};
pub use xref::ObjectId;

pub type Result<T> = std::result::Result<T, Error>;
