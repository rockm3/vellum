use vellum_pdf::{Matrix, Rect};

pub struct RenderTarget {
    pub width:  u32,
    pub height: u32,
    /// Raw RGBA pixels, row-major, top-left origin
    pub pixels: Vec<u8>,
}

impl RenderTarget {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, pixels: vec![0u8; (width * height * 4) as usize] }
    }
}

pub trait RenderBackend {
    fn begin_page(&mut self, width: f32, height: f32);
    fn end_page(&mut self) -> RenderTarget;
    fn set_transform(&mut self, matrix: Matrix);
    fn fill_rect(&mut self, rect: Rect, color: [u8; 4]);
}
