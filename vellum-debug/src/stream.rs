use lopdf::{content::Content, Object};

/// 将 PDF 内容流以带注释的可读格式打印到 stdout。
///
/// 输出示例：
/// ```text
/// ─── 第 0 页内容流  (10 条指令) ──────────────────────────────────────
///    #   指令                                                 说明
///   ──   ───────────────────────────────────────────────────  ──────────────────────
///    1   q                                                    保存图形状态
///    2   0.9 0.9 0.9 rg                                       设置填充 RGB 颜色  → r=0.9 g=0.9 b=0.9
///    3   72 708 468 36 re                                     添加矩形路径  → x=72 y=708 w=468 h=36
///    4   f                                                    填充路径（非零绕数）
///    5   Q                                                    恢复图形状态
///    6   ┌ BT                                                 开始文本对象
///    7   │   /Helvetica 14 Tf                                 设置字体和大小  → 字体=/Helvetica 大小=14pt
///    8   │   72 720 Td                                        文本位置偏移  → dx=72 dy=720
///    9   │   (Hello!) Tj                                      显示字符串  → "Hello!"
///   10   └ ET                                                 结束文本对象
/// ```
pub struct StreamPrinter;

impl StreamPrinter {
    pub fn new() -> Self { Self }

    /// 直接打印到 stdout。
    pub fn print(&self, page_index: u32, bytes: &[u8]) {
        print!("{}", self.render(page_index, bytes));
    }

    /// 渲染为字符串，便于测试和文件输出。
    pub fn render(&self, page_index: u32, bytes: &[u8]) -> String {
        use std::fmt::Write as _;

        let ops = match Content::decode(bytes) {
            Ok(c) => c.operations,
            Err(e) => return format!("  [内容流解析失败: {e}]\n"),
        };

        let divider = "─".repeat(72);
        let mut out = String::new();
        let _ = writeln!(out, "{divider}");
        let _ = writeln!(out, " 第 {page_index} 页内容流  ({} 条指令)", ops.len());
        let _ = writeln!(out, "{divider}");
        let _ = writeln!(out, "  {:>4}  {:<52}  {}", "#", "指令", "说明");
        let _ = writeln!(out, "  {}  {}  {}", "─".repeat(4), "─".repeat(52), "─".repeat(24));

        let mut in_bt = false;

        for (i, op) in ops.iter().enumerate() {
            let (prefix, inner_indent) = match op.operator.as_str() {
                "BT"  => ("┌ ", ""),
                "ET"  => ("└ ", ""),
                _ if in_bt => ("│ ", "  "),
                _ => ("  ", ""),
            };

            let operands_str = format_operands(&op.operands);
            let cmd = if operands_str.is_empty() {
                format!("{prefix}{inner_indent}{}", op.operator)
            } else {
                format!("{prefix}{inner_indent}{operands_str} {}", op.operator)
            };

            let desc  = operator_desc(&op.operator);
            let extra = operator_extra(&op.operator, &op.operands);
            let annotation = if extra.is_empty() {
                desc.to_string()
            } else {
                format!("{desc}  → {extra}")
            };

            let _ = writeln!(out, "  {:>4}  {:<52}  {}", i + 1, cmd, annotation);

            match op.operator.as_str() {
                "BT" => in_bt = true,
                "ET" => in_bt = false,
                _ => {}
            }
        }

        let _ = writeln!(out, "{divider}");
        out
    }
}

impl Default for StreamPrinter {
    fn default() -> Self { Self::new() }
}

// ─ 操作符描述 ─────────────────────────────────────────────────────────────────

fn operator_desc(op: &str) -> &'static str {
    match op {
        // 文本对象
        "BT"  => "开始文本对象",
        "ET"  => "结束文本对象",
        // 字体与文本状态
        "Tf"  => "设置字体和大小",
        "Tc"  => "设置字符间距",
        "Tw"  => "设置词间距",
        "Tz"  => "设置水平缩放比例",
        "TL"  => "设置行距",
        "Tr"  => "设置文本渲染模式",
        "Ts"  => "设置文本上升量",
        // 文本位置
        "Td"  => "文本位置偏移",
        "TD"  => "文本位置偏移并更新行距",
        "Tm"  => "设置文本矩阵和位置",
        "T*"  => "移动到下一行",
        // 文本显示
        "Tj"  => "显示字符串",
        "TJ"  => "显示字符串（含字符间距调整）",
        "'"   => "换行并显示字符串",
        "\""  => "设置间距、换行并显示字符串",
        // 图形状态
        "q"   => "保存图形状态",
        "Q"   => "恢复图形状态",
        "cm"  => "修改当前变换矩阵",
        "w"   => "设置线宽",
        "J"   => "设置线端样式",
        "j"   => "设置线拐角样式",
        "M"   => "设置斜接限制",
        "d"   => "设置虚线样式",
        "ri"  => "设置颜色渲染意图",
        "i"   => "设置平滑度容差",
        "gs"  => "应用扩展图形状态字典",
        // 路径构造
        "m"   => "路径起点",
        "l"   => "直线段",
        "c"   => "三次贝塞尔曲线",
        "v"   => "贝塞尔曲线（首控制点同当前点）",
        "y"   => "贝塞尔曲线（末控制点同终点）",
        "h"   => "关闭当前子路径",
        "re"  => "添加矩形路径",
        // 路径绘制
        "S"   => "描边路径",
        "s"   => "关闭并描边",
        "f"   => "填充路径（非零绕数）",
        "F"   => "填充路径（非零绕数，同 f）",
        "f*"  => "填充路径（奇偶规则）",
        "B"   => "填充并描边（非零绕数）",
        "B*"  => "填充并描边（奇偶规则）",
        "b"   => "关闭、填充并描边",
        "b*"  => "关闭、奇偶填充并描边",
        "n"   => "结束路径（不填充不描边）",
        // 裁切路径
        "W"   => "设置裁切路径（非零绕数）",
        "W*"  => "设置裁切路径（奇偶规则）",
        // 色彩空间
        "CS"  => "设置描边色彩空间",
        "cs"  => "设置填充色彩空间",
        "SC"  => "设置描边颜色",
        "SCN" => "设置描边颜色（扩展）",
        "sc"  => "设置填充颜色",
        "scn" => "设置填充颜色（扩展）",
        "G"   => "设置描边灰度",
        "g"   => "设置填充灰度",
        "RG"  => "设置描边 RGB 颜色",
        "rg"  => "设置填充 RGB 颜色",
        "K"   => "设置描边 CMYK 颜色",
        "k"   => "设置填充 CMYK 颜色",
        // XObject / 图像
        "Do"  => "绘制 XObject（图片/表单）",
        "BI"  => "开始内联图像",
        "ID"  => "内联图像数据开始",
        "EI"  => "结束内联图像",
        // 渐变
        "sh"  => "绘制渐变填充",
        // 标记内容
        "BMC" => "开始标记内容",
        "BDC" => "开始带属性的标记内容",
        "EMC" => "结束标记内容",
        "MP"  => "已定义标记点",
        "DP"  => "已定义带属性的标记点",
        // 兼容性
        "BX"  => "开始兼容性节",
        "EX"  => "结束兼容性节",
        _     => "（未知指令）",
    }
}

// ─ 操作符额外上下文信息 ────────────────────────────────────────────────────────

fn operator_extra(op: &str, o: &[Object]) -> String {
    match op {
        "Tf" if o.len() >= 2 => {
            format!("字体={}  大小={}pt", fmt(& o[0]), fmt(&o[1]))
        }
        "Td" | "TD" if o.len() >= 2 => {
            format!("dx={}  dy={}", fmt(&o[0]), fmt(&o[1]))
        }
        "Tm" if o.len() >= 6 => {
            // 文本矩阵 [a b c d e f]，e/f 是平移分量
            format!("位置=({}, {})", fmt(&o[4]), fmt(&o[5]))
        }
        "Tj" if !o.is_empty() => {
            if let Object::String(bytes, _) = &o[0] {
                format!("\"{}\"", decode_pdf_string(bytes))
            } else { String::new() }
        }
        "TJ" if !o.is_empty() => {
            if let Object::Array(arr) = &o[0] {
                let text: String = arr.iter().filter_map(|item| {
                    if let Object::String(b, _) = item { Some(decode_pdf_string(b)) } else { None }
                }).collect();
                format!("\"{}\"", text)
            } else { String::new() }
        }
        "re" if o.len() >= 4 => {
            format!("x={}  y={}  w={}  h={}", fmt(&o[0]), fmt(&o[1]), fmt(&o[2]), fmt(&o[3]))
        }
        "rg" | "RG" if o.len() >= 3 => {
            format!("r={}  g={}  b={}", fmt(&o[0]), fmt(&o[1]), fmt(&o[2]))
        }
        "k" | "K" if o.len() >= 4 => {
            format!("c={}  m={}  y={}  k={}", fmt(&o[0]), fmt(&o[1]), fmt(&o[2]), fmt(&o[3]))
        }
        "G" | "g" if !o.is_empty() => {
            format!("灰度={}", fmt(&o[0]))
        }
        "cm" if o.len() >= 6 => {
            let vals: Vec<_> = o.iter().take(6).map(fmt).collect();
            format!("[{}]", vals.join("  "))
        }
        "w" if !o.is_empty() => {
            format!("{}pt", fmt(&o[0]))
        }
        "Do" if !o.is_empty() => {
            format!("引用={}", fmt(&o[0]))
        }
        "m" | "l" if o.len() >= 2 => {
            format!("({}, {})", fmt(&o[0]), fmt(&o[1]))
        }
        "Tc" | "Tw" | "TL" | "Tz" | "Ts" if !o.is_empty() => {
            fmt(&o[0])
        }
        _ => String::new(),
    }
}

// ─ 格式化工具 ─────────────────────────────────────────────────────────────────

fn format_operands(operands: &[Object]) -> String {
    operands.iter().map(fmt).collect::<Vec<_>>().join(" ")
}

fn fmt(obj: &Object) -> String {
    match obj {
        Object::Integer(n)      => n.to_string(),
        Object::Real(f)         => fmt_real(*f as f64),
        Object::Boolean(b)      => b.to_string(),
        Object::Name(name)      => format!("/{}", String::from_utf8_lossy(name)),
        Object::String(b, _)    => format!("({})", decode_pdf_string(b)),
        Object::Array(arr)      => {
            let items: Vec<_> = arr.iter().map(fmt).collect();
            format!("[{}]", items.join(" "))
        }
        Object::Null            => "null".to_string(),
        Object::Dictionary(_)   => "{dict}".to_string(),
        Object::Stream(_)       => "{stream}".to_string(),
        Object::Reference(id)   => format!("{} {} R", id.0, id.1),
    }
}

fn fmt_real(f: f64) -> String {
    // 去掉多余的尾零：12.0 → "12"，0.5000 → "0.5"，3.1416 → "3.1416"
    let s = format!("{:.4}", f);
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    s.to_string()
}

/// 尝试按 UTF-16BE（BOM 0xFE 0xFF）、UTF-8、Latin-1 依序解码 PDF 字符串字节。
fn decode_pdf_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let utf16: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
            .collect();
        return String::from_utf16_lossy(&utf16);
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    bytes.iter().map(|&b| b as char).collect()
}
