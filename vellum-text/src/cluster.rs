use vellum_pdf::{Point, Rect};
use crate::glyph::RawGlyph;

// ── 数据结构 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Word {
    pub glyphs: Vec<RawGlyph>,
    pub bounds: Rect,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub words: Vec<Word>,
    pub bounds: Rect,
    /// 排版基线的 Y 坐标（页面坐标系，原点在左下角）
    pub baseline_y: f32,
}

#[derive(Debug, Clone)]
pub struct TextBlock {
    pub lines: Vec<Line>,
    pub bounds: Rect,
    /// 在页面阅读顺序中的位置（从 0 开始）
    pub reading_order: usize,
}

// ── 阈值常量 ──────────────────────────────────────────────────────────────────

/// 同一行的 Y 坐标最大偏差（pt）
const LINE_Y_TOL: f32 = 2.0;

/// 词间距阈值系数：origin_to_origin > WORD_GAP_K * font_size → 新词
const WORD_GAP_K: f32 = 1.0;

/// 块间距阈值系数：baseline_gap > BLOCK_GAP_K * font_size → 新块
const BLOCK_GAP_K: f32 = 1.5;

// ── 入口 ──────────────────────────────────────────────────────────────────────

/// 将原始字形列表聚类为文本块层次结构。
///
/// 流程：
///   1. 过滤无 Unicode 映射或字号为零的字形
///   2. 按 Y 降序、X 升序排列（上→下、左→右）
///   3. 按 Y 邻近度分组为行
///   4. 行内按水平间距分组为词
///   5. 按行间距分组为块
pub fn cluster(raw: Vec<RawGlyph>) -> Vec<TextBlock> {
    // glyph_id == 0 是空字形（CID 字体子集占位符），直接丢弃。
    // 其余保留：无 Unicode 映射的非零字形在 flush_word 里用 [XX] 占位显示。
    let glyphs: Vec<RawGlyph> = raw.into_iter()
        .filter(|g| g.font_size > 0.0 && g.glyph_id != 0)
        .collect();
    if glyphs.is_empty() { return vec![]; }

    let sorted = sort_glyphs(glyphs);
    let line_groups = group_by_y(sorted);

    let lines: Vec<Line> = line_groups
        .into_iter()
        .filter_map(build_line)
        .collect();

    if lines.is_empty() { return vec![]; }

    group_into_blocks(lines)
}

// ── 排序与分行 ────────────────────────────────────────────────────────────────

fn sort_glyphs(mut glyphs: Vec<RawGlyph>) -> Vec<RawGlyph> {
    glyphs.sort_by(|a, b| {
        b.origin.y
            .partial_cmp(&a.origin.y).unwrap_or(std::cmp::Ordering::Equal)
            .then(a.origin.x.partial_cmp(&b.origin.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    glyphs
}

/// 将按 Y 降序排列的字形序列按 Y 近邻分组为行候选组。
fn group_by_y(sorted: Vec<RawGlyph>) -> Vec<Vec<RawGlyph>> {
    let mut groups: Vec<Vec<RawGlyph>> = Vec::new();
    for g in sorted {
        if let Some(last) = groups.last_mut() {
            if (g.origin.y - last[0].origin.y).abs() <= LINE_Y_TOL {
                last.push(g);
                continue;
            }
        }
        groups.push(vec![g]);
    }
    groups
}

// ── 行构建 ────────────────────────────────────────────────────────────────────

fn build_line(mut glyphs: Vec<RawGlyph>) -> Option<Line> {
    if glyphs.is_empty() { return None; }

    // 行内再按 X 升序排列（group_by_y 已按 Y 降序排，同 Y 内可能顺序混乱）
    glyphs.sort_by(|a, b| a.origin.x.partial_cmp(&b.origin.x).unwrap_or(std::cmp::Ordering::Equal));

    let baseline_y = glyphs[0].origin.y;
    let words = build_words(glyphs);
    if words.is_empty() { return None; }

    let bounds = bounding_rect(words.iter().map(|w| w.bounds));
    Some(Line { words, bounds, baseline_y })
}

// ── 词构建 ────────────────────────────────────────────────────────────────────

fn build_words(glyphs: Vec<RawGlyph>) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    let mut current: Vec<RawGlyph> = Vec::new();

    for g in glyphs {
        if let Some(prev) = current.last() {
            let dx = g.origin.x - prev.origin.x;
            if dx > WORD_GAP_K * prev.font_size {
                flush_word(&mut current, &mut words);
            }
        }
        current.push(g);
    }
    flush_word(&mut current, &mut words);
    words
}

/// 将积累的字形转为 Word，纯空白词直接丢弃。
///
/// 无 Unicode 映射的非零字形输出为 `[XX]` 占位符，保留可见痕迹。
fn flush_word(cur: &mut Vec<RawGlyph>, out: &mut Vec<Word>) {
    if cur.is_empty() { return; }
    let glyphs: Vec<RawGlyph> = std::mem::take(cur);
    let text: String = glyphs.iter()
        .map(|g| match g.unicode {
            Some(c) if !c.is_whitespace() => c.to_string(),
            Some(_)                        => String::new(),
            None                           => format!("[{:02X}]", g.glyph_id),
        })
        .collect();
    if text.is_empty() { return; }
    let bounds = bounding_rect(glyphs.iter().map(|g| g.bounds));
    out.push(Word { glyphs, bounds, text });
}

// ── 块构建 ────────────────────────────────────────────────────────────────────

/// 将已排序的行列表（Y 降序，即上→下）按行间距分组为文本块。
fn group_into_blocks(lines: Vec<Line>) -> Vec<TextBlock> {
    let mut blocks: Vec<TextBlock> = Vec::new();
    let mut current: Vec<Line> = Vec::new();

    for line in lines {
        if let Some(prev) = current.last() {
            let gap       = (prev.baseline_y - line.baseline_y).abs();
            let ref_fs    = max_font_size(prev).max(1.0);
            if gap > BLOCK_GAP_K * ref_fs {
                flush_block(&mut current, &mut blocks);
            }
        }
        current.push(line);
    }
    flush_block(&mut current, &mut blocks);
    blocks
}

fn flush_block(cur: &mut Vec<Line>, out: &mut Vec<TextBlock>) {
    if cur.is_empty() { return; }
    let lines: Vec<Line> = std::mem::take(cur);
    let order  = out.len();
    let bounds = bounding_rect(lines.iter().map(|l| l.bounds));
    out.push(TextBlock { lines, bounds, reading_order: order });
}

// ── 工具函数 ──────────────────────────────────────────────────────────────────

fn bounding_rect(rects: impl Iterator<Item = Rect>) -> Rect {
    let (mut x0, mut y0) = (f32::INFINITY,     f32::INFINITY);
    let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut any = false;
    for r in rects {
        any   = true;
        x0    = x0.min(r.min.x);
        y0    = y0.min(r.min.y);
        x1    = x1.max(r.max.x);
        y1    = y1.max(r.max.y);
    }
    if any {
        Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } }
    } else {
        Rect { min: Point { x: 0.0, y: 0.0 }, max: Point { x: 0.0, y: 0.0 } }
    }
}

fn max_font_size(line: &Line) -> f32 {
    line.words.iter()
        .flat_map(|w| w.glyphs.iter())
        .map(|g| g.font_size)
        .fold(0.0f32, f32::max)
}

// ── 单元测试 ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph::MappingMethod;

    fn g(x: f32, y: f32, fs: f32, ch: char) -> RawGlyph {
        RawGlyph {
            glyph_id:       ch as u32,
            unicode:        Some(ch),
            mapping_method: MappingMethod::ToUnicode,
            origin:         Point { x, y },
            bounds:         Rect::new(x, y, fs * 0.6, fs),
            font_size:      fs,
            font_name:      "/F1".into(),
        }
    }

    fn null_g(x: f32, y: f32, fs: f32) -> RawGlyph {
        RawGlyph {
            glyph_id:       0,
            unicode:        None,
            mapping_method: MappingMethod::Unmapped,
            origin:         Point { x, y },
            bounds:         Rect::new(x, y, 0.0, fs),
            font_size:      fs,
            font_name:      "/F1".into(),
        }
    }

    #[test]
    fn empty_gives_no_blocks() {
        assert!(cluster(vec![]).is_empty());
    }

    #[test]
    fn glyph_id_zero_filtered_out() {
        // glyph_id == 0 是 CID 字体占位符，无论 unicode 状态如何都应丢弃
        let glyphs = vec![null_g(0.0, 100.0, 12.0), null_g(10.0, 100.0, 12.0)];
        assert!(cluster(glyphs).is_empty());
    }

    #[test]
    fn unmapped_nonzero_glyph_shows_as_placeholder() {
        // glyph_id != 0 但无 Unicode 映射 → 词文字显示为 [XX]
        let mut ug = null_g(0.0, 100.0, 12.0);
        ug.glyph_id = 0xB3; // 非零、无映射
        let glyphs = vec![ug];
        let blocks = cluster(glyphs);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lines[0].words[0].text, "[B3]");
    }

    #[test]
    fn single_glyph_forms_one_block_one_line_one_word() {
        let blocks = cluster(vec![g(50.0, 200.0, 12.0, 'A')]);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lines.len(), 1);
        assert_eq!(blocks[0].lines[0].words.len(), 1);
        assert_eq!(blocks[0].lines[0].words[0].text, "A");
    }

    #[test]
    fn close_glyphs_same_y_form_one_line() {
        // A(0), B(6), C(12)，间距 6 < 1.0*12 → 同一词
        let glyphs = vec![
            g(0.0,  100.0, 12.0, 'A'),
            g(6.0,  100.0, 12.0, 'B'),
            g(12.0, 100.0, 12.0, 'C'),
        ];
        let blocks = cluster(glyphs);
        assert_eq!(blocks.len(), 1);
        let line = &blocks[0].lines[0];
        assert_eq!(line.words.len(), 1);
        assert_eq!(line.words[0].text, "ABC");
    }

    #[test]
    fn large_gap_splits_into_two_words() {
        // A(0), B(50)，间距 50 > 1.0*12 = 12 → 两词
        let glyphs = vec![g(0.0, 100.0, 12.0, 'A'), g(50.0, 100.0, 12.0, 'B')];
        let blocks = cluster(glyphs);
        assert_eq!(blocks[0].lines[0].words.len(), 2);
        assert_eq!(blocks[0].lines[0].words[0].text, "A");
        assert_eq!(blocks[0].lines[0].words[1].text, "B");
    }

    #[test]
    fn different_y_forms_different_lines() {
        // y=100 和 y=80，间距 20 < BLOCK_GAP_K * 12 = 18? → 同一块
        // 20 > 18 → 两块（取决于阈值）
        // 用不太贴近的值：y=100 和 y=50，间距 50 > 1.5*12=18 → 两块
        let glyphs = vec![
            g(0.0, 100.0, 12.0, 'A'),
            g(0.0,  50.0, 12.0, 'B'),
        ];
        let blocks = cluster(glyphs);
        // 不管是 1 块 2 行还是 2 块，只要有两行
        let total_lines: usize = blocks.iter().map(|b| b.lines.len()).sum();
        assert_eq!(total_lines, 2);
        // A 行的 baseline_y 更大（上方），B 在下方
        let all_lines: Vec<&Line> = blocks.iter().flat_map(|b| b.lines.iter()).collect();
        assert!(all_lines[0].baseline_y > all_lines[1].baseline_y,
            "上方行应先出现");
    }

    #[test]
    fn large_line_gap_splits_blocks() {
        // y=100 到 y=50 间距 50 > 1.5*12=18 → 两块
        let glyphs = vec![
            g(0.0, 100.0, 12.0, 'A'),
            g(0.0,  50.0, 12.0, 'B'),
        ];
        let blocks = cluster(glyphs);
        assert_eq!(blocks.len(), 2, "行间距 50 > 18，应分两块");
    }

    #[test]
    fn small_line_gap_stays_same_block() {
        // y=100 到 y=86，间距 14 < 1.5*12=18 → 同一块
        let glyphs = vec![
            g(0.0, 100.0, 12.0, 'A'),
            g(0.0,  86.0, 12.0, 'B'),
        ];
        let blocks = cluster(glyphs);
        assert_eq!(blocks.len(), 1, "行间距 14 < 18，应同一块");
        assert_eq!(blocks[0].lines.len(), 2);
    }

    #[test]
    fn y_tolerance_groups_near_glyphs_into_one_line() {
        // Y 偏差 1.5 ≤ LINE_Y_TOL=2.0 → 同一行
        let glyphs = vec![
            g(0.0, 100.0,  12.0, 'A'),
            g(6.0,  98.6,  12.0, 'B'),
        ];
        let blocks  = cluster(glyphs);
        let n_lines: usize = blocks.iter().map(|b| b.lines.len()).sum();
        assert_eq!(n_lines, 1, "Y 偏差 1.4 ≤ 容差 2.0，应同一行");
    }

    #[test]
    fn reading_order_assigned_sequentially() {
        let glyphs = vec![
            g(0.0, 100.0, 12.0, 'A'),
            g(0.0,  50.0, 12.0, 'B'),
        ];
        let blocks = cluster(glyphs);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].reading_order, 0);
        assert_eq!(blocks[1].reading_order, 1);
    }

    #[test]
    fn blocks_ordered_top_to_bottom() {
        // 字形乱序输入，输出应按 Y 降序（上→下）排列
        let glyphs = vec![
            g(0.0,  50.0, 12.0, 'B'),
            g(0.0, 100.0, 12.0, 'A'),
        ];
        let blocks = cluster(glyphs);
        // 第一块 baseline_y 应更大（更高）
        assert!(blocks[0].lines[0].baseline_y > blocks[1].lines[0].baseline_y);
        assert_eq!(blocks[0].lines[0].words[0].text, "A");
        assert_eq!(blocks[1].lines[0].words[0].text, "B");
    }

    #[test]
    fn space_glyphs_do_not_appear_in_word_text() {
        let glyphs = vec![
            g(0.0,  100.0, 12.0, 'H'),
            g(6.0,  100.0, 12.0, 'i'),
            g(12.0, 100.0, 12.0, ' '),
            g(18.0, 100.0, 12.0, '!'),
        ];
        let blocks = cluster(glyphs);
        // 所有词的文字均不含空格
        let all_text: String = blocks.iter()
            .flat_map(|b| b.lines.iter())
            .flat_map(|l| l.words.iter())
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(!all_text.contains(' '), "词文字不应含空格，实际='{all_text}'");
    }

    #[test]
    fn bounds_cover_all_glyphs() {
        // 词的 bounds 应包含所有字形
        let glyphs = vec![
            g(10.0, 100.0, 12.0, 'A'),
            g(20.0, 100.0, 12.0, 'B'),
            g(30.0, 100.0, 12.0, 'C'),
        ];
        let blocks = cluster(glyphs);
        let word = &blocks[0].lines[0].words[0];
        assert!(word.bounds.min.x <= 10.0 + 0.01);
        assert!(word.bounds.max.x >= 30.0 + 12.0 * 0.6 - 0.01);
    }

    #[test]
    fn mixed_null_and_valid_glyphs() {
        let glyphs = vec![
            null_g(0.0,  100.0, 12.0),
            g(10.0, 100.0, 12.0, 'A'),
            null_g(20.0, 100.0, 12.0),
            g(30.0, 100.0, 12.0, 'B'),
        ];
        let blocks = cluster(glyphs);
        assert_eq!(blocks.len(), 1);
        // A 和 B 之间真实间距 30-10=20 > 1.0*12=12 → 两词
        assert_eq!(blocks[0].lines[0].words.len(), 2);
        assert_eq!(blocks[0].lines[0].words[0].text, "A");
        assert_eq!(blocks[0].lines[0].words[1].text, "B");
    }
}
