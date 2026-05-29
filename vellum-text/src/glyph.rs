use vellum_pdf::{Point, Rect};

/// 字形 Unicode 映射所采用的解析路径。
/// 每个 `RawGlyph` 均记录此字段，便于调试工具展示映射来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingMethod {
    /// PDF ToUnicode CMap 流 — 最可靠的来源
    ToUnicode,
    /// Font Encoding 字典 + Adobe Glyph List (AGL) 查表
    EncodingAgl,
    /// CIDFont CMap — 用于 CJK 及其他多字节编码
    CidCmap,
    /// 字体名或字形名启发式推断 — 最后手段
    Heuristic,
    /// 未找到映射，字形无法表示为 Unicode
    Unmapped,
}

#[derive(Debug, Clone)]
pub struct RawGlyph {
    pub glyph_id: u32,
    pub unicode: Option<char>,
    pub mapping_method: MappingMethod,
    /// 字形原点坐标，页面坐标系（左下角为原点，PDF 惯例）
    pub origin: Point,
    /// 页面坐标系中的紧包围盒
    pub bounds: Rect,
    pub font_size: f32,
    pub font_name: String,
}
