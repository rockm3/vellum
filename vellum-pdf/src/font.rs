use std::collections::HashMap;

/// 渲染所需的字体数据，已从 PDF 对象中提取为纯数据（不含 lopdf 类型）。
#[derive(Debug, Clone)]
pub enum Font {
    /// Type3 字体：每个字形是一段 PDF 内容流（CharProc）。
    Type3 {
        /// 字形空间 → 文本空间的变换矩阵 `[a b c d e f]`。
        font_matrix: [f32; 6],
        /// 单字节编码 → 该字形的 CharProc 内容流字节（已解压）。
        char_procs: HashMap<u8, Vec<u8>>,
        /// 单字节编码 → 字形宽度（字形空间单位）。
        widths: HashMap<u8, f32>,
    },
    /// 嵌入 TrueType/OpenType 字体程序（FontFile2）。
    TrueType {
        /// 字体程序原始字节。
        program: Vec<u8>,
        /// 是否双字节编码（Type0 / Identity-H）。
        two_byte: bool,
    },
    /// 暂不支持的字体类型（仍占位，便于调试时显示 [XX]）。
    Unsupported,
}

/// 一页中按资源名（如 `"/F4"`）索引的字体集合。
pub type PageFonts = HashMap<String, Font>;
