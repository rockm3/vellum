pub mod console;
mod json;
mod stream;
mod svg;

pub use console::ConsoleParseSink;
pub use json::JsonTraceSink;
pub use stream::StreamPrinter;
pub use svg::SvgOverlaySink;
