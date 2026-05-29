use std::cmp::Ordering;
use crate::cluster::TextBlock;

// ── 公共接口 ──────────────────────────────────────────────────────────────────

/// 用 XY-Cut 算法为文本块列表填充 `reading_order` 字段。
///
/// 算法概述：
///   1. 在所有块的包围盒中寻找"水平间隙"（任何块都不跨越的 Y 坐标）
///   2. 若找到：上方块先于下方块递归处理
///   3. 若无水平间隙：寻找"垂直间隙"，左列先于右列
///   4. 若无任何间隙：按 Y 降序、X 升序排列（上→下、左→右）
pub fn assign_reading_order(blocks: &mut [TextBlock]) {
    if blocks.is_empty() { return; }
    let n = blocks.len();
    let ordered = xy_cut((0..n).collect::<Vec<_>>().as_slice(), blocks);
    for (order, &idx) in ordered.iter().enumerate() {
        blocks[idx].reading_order = order;
    }
}

/// 按 `reading_order` 提取全页纯文本（块间换行，行内词间空格）。
pub fn extract_text(blocks: &[TextBlock]) -> String {
    let mut sorted: Vec<&TextBlock> = blocks.iter().collect();
    sorted.sort_by_key(|b| b.reading_order);
    sorted.iter()
        .map(|b| {
            b.lines.iter()
                .map(|l| l.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ── XY-Cut 递归实现 ───────────────────────────────────────────────────────────

/// 返回 `indices` 按阅读顺序排列后的序列。
fn xy_cut(indices: &[usize], blocks: &[TextBlock]) -> Vec<usize> {
    if indices.len() <= 1 { return indices.to_vec(); }

    // 优先尝试水平切割（上→下阅读）
    if let Some(cut_y) = find_h_cut(indices, blocks) {
        let top: Vec<usize> = indices.iter().copied()
            .filter(|&i| blocks[i].bounds.min.y > cut_y)
            .collect();
        let bot: Vec<usize> = indices.iter().copied()
            .filter(|&i| blocks[i].bounds.max.y <= cut_y)
            .collect();
        if !top.is_empty() && !bot.is_empty() && top.len() + bot.len() == indices.len() {
            let mut r = xy_cut(&top, blocks);
            r.extend(xy_cut(&bot, blocks));
            return r;
        }
    }

    // 其次尝试垂直切割（左→右阅读）
    if let Some(cut_x) = find_v_cut(indices, blocks) {
        let left: Vec<usize> = indices.iter().copied()
            .filter(|&i| blocks[i].bounds.max.x <= cut_x)
            .collect();
        let right: Vec<usize> = indices.iter().copied()
            .filter(|&i| blocks[i].bounds.min.x > cut_x)
            .collect();
        if !left.is_empty() && !right.is_empty() && left.len() + right.len() == indices.len() {
            let mut r = xy_cut(&left, blocks);
            r.extend(xy_cut(&right, blocks));
            return r;
        }
    }

    // 无法切割：退回 Y 降序、X 升序兜底排序
    let mut sorted = indices.to_vec();
    sorted.sort_by(|&a, &b| {
        // 先按中心 Y 降序（高 Y = 页面上方 = 先读）
        let cy_a = (blocks[a].bounds.min.y + blocks[a].bounds.max.y) / 2.0;
        let cy_b = (blocks[b].bounds.min.y + blocks[b].bounds.max.y) / 2.0;
        cy_b.partial_cmp(&cy_a)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                // 同 Y 按中心 X 升序（左→右）
                let cx_a = (blocks[a].bounds.min.x + blocks[a].bounds.max.x) / 2.0;
                let cx_b = (blocks[b].bounds.min.x + blocks[b].bounds.max.x) / 2.0;
                cx_a.partial_cmp(&cx_b).unwrap_or(Ordering::Equal)
            })
    });
    sorted
}

// ── 间隙搜索 ─────────────────────────────────────────────────────────────────

/// 寻找水平切割线 Y，使每个块完全位于切割线上方或下方（不跨越）。
///
/// 使用扫描线法（从上→下 = Y 降序）：
/// - 每个块在其 max_y（顶边）处"进入"，在 min_y（底边）处"离开"
/// - 当活跃块数为零且距下一块顶边的间隙 > 1pt 时，找到有效切割线
fn find_h_cut(indices: &[usize], blocks: &[TextBlock]) -> Option<f32> {
    let mut events = collect_events(indices, blocks, /* vertical = */ false);
    // Y 降序；同 Y 时"进入"（+1）先于"离开"（-1），避免虚假间隙
    events.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal).then(b.1.cmp(&a.1)));
    sweep_for_gap(&events)
}

/// 寻找垂直切割线 X，使每个块完全位于切割线左方或右方（不跨越）。
///
/// 使用扫描线法（从左→右 = X 升序）：
/// - 每个块在其 min_x（左边）处"进入"，在 max_x（右边）处"离开"
fn find_v_cut(indices: &[usize], blocks: &[TextBlock]) -> Option<f32> {
    let mut events = collect_events(indices, blocks, /* vertical = */ true);
    // X 升序；同 X 时"进入"（+1）先于"离开"（-1）
    events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal).then(b.1.cmp(&a.1)));
    sweep_for_gap(&events)
}

/// 生成事件序列：(坐标, +1进入/-1离开)
fn collect_events(indices: &[usize], blocks: &[TextBlock], vertical: bool) -> Vec<(f32, i32)> {
    let mut ev = Vec::with_capacity(indices.len() * 2);
    for &i in indices {
        let b = &blocks[i].bounds;
        let (enter, exit) = if vertical {
            (b.min.x, b.max.x)   // 从左到右
        } else {
            (b.max.y, b.min.y)   // 从上（高 Y）到下（低 Y）
        };
        ev.push((enter,  1));
        ev.push((exit,  -1));
    }
    ev
}

/// 在已排序的事件序列中寻找活跃块为零时的间隙。
fn sweep_for_gap(events: &[(f32, i32)]) -> Option<f32> {
    let mut active: i32 = 0;
    let mut gap_edge: Option<f32> = None; // 间隙开始处的坐标

    for &(coord, delta) in events {
        // 即将进入新块 (+1)：检查是否存在间隙
        if delta == 1 {
            if let Some(ge) = gap_edge {
                // 根据排序方式判断间隙方向：
                // 水平（降序）时 ge > coord，垂直（升序）时 coord > ge
                let gap = (ge - coord).abs();
                if gap > 1.0 {
                    return Some((ge + coord) / 2.0);
                }
                gap_edge = None;
            }
        }
        active += delta;
        if active == 0 { gap_edge = Some(coord); }
    }
    None
}

// ── 单元测试 ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use vellum_pdf::{Point, Rect};
    use crate::cluster::{Line, TextBlock, Word};
    use crate::glyph::{MappingMethod, RawGlyph};

    fn block(x0: f32, y0: f32, x1: f32, y1: f32) -> TextBlock {
        TextBlock {
            lines: vec![],
            bounds: Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } },
            reading_order: 0,
        }
    }

    fn block_with_text(x0: f32, y0: f32, x1: f32, y1: f32, text: &str) -> TextBlock {
        let g = RawGlyph {
            glyph_id: 0, unicode: None,
            mapping_method: MappingMethod::Unmapped,
            origin: Point { x: x0, y: y0 },
            bounds: Rect::new(x0, y0, 1.0, 1.0),
            font_size: 12.0, font_name: String::new(),
        };
        let w = Word {
            glyphs: vec![g],
            bounds: Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } },
            text: text.to_string(),
        };
        let l = Line {
            words: vec![w],
            bounds: Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } },
            baseline_y: (y0 + y1) / 2.0,
        };
        TextBlock {
            lines: vec![l],
            bounds: Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } },
            reading_order: 0,
        }
    }

    // ── 基本情形 ──────────────────────────────────────────────────────────────

    #[test]
    fn single_block_gets_order_zero() {
        let mut blocks = vec![block(0.0, 0.0, 100.0, 100.0)];
        assign_reading_order(&mut blocks);
        assert_eq!(blocks[0].reading_order, 0);
    }

    #[test]
    fn two_vertical_blocks_top_first() {
        // 上方块 y=50..100，下方块 y=0..40（间距 10）→ 上方先读
        let mut blocks = vec![
            block(0.0, 0.0,  100.0, 40.0),  // 下（索引 0）
            block(0.0, 50.0, 100.0, 100.0), // 上（索引 1）
        ];
        assign_reading_order(&mut blocks);
        assert_eq!(blocks[1].reading_order, 0, "上方块应先读");
        assert_eq!(blocks[0].reading_order, 1, "下方块应后读");
    }

    #[test]
    fn two_horizontal_blocks_left_first() {
        // 无水平间隙（同高），有垂直间隙 → 左先右后
        let mut blocks = vec![
            block(60.0, 0.0, 110.0, 100.0), // 右（索引 0）
            block(0.0,  0.0,  50.0, 100.0), // 左（索引 1）
        ];
        assign_reading_order(&mut blocks);
        assert_eq!(blocks[1].reading_order, 0, "左块应先读");
        assert_eq!(blocks[0].reading_order, 1, "右块应后读");
    }

    #[test]
    fn two_by_two_grid_row_major() {
        // 2×2 布局 → 行优先（TL=0, TR=1, BL=2, BR=3）
        let mut blocks = vec![
            block(0.0,  0.0,  50.0,  40.0),  // BL (0)
            block(60.0, 0.0,  110.0, 40.0),  // BR (1)
            block(0.0,  50.0, 50.0,  100.0), // TL (2)
            block(60.0, 50.0, 110.0, 100.0), // TR (3)
        ];
        assign_reading_order(&mut blocks);
        let tl = blocks[2].reading_order;
        let tr = blocks[3].reading_order;
        let bl = blocks[0].reading_order;
        let br = blocks[1].reading_order;
        assert!(tl < tr, "TL 应先于 TR");
        assert!(tl < bl, "TL 应先于 BL");
        assert!(tr < br, "TR 应先于 BR");
        assert!(bl < br, "BL 应先于 BR");
        // 行优先：TL=0, TR=1, BL=2, BR=3
        assert_eq!(tl, 0);
        assert_eq!(tr, 1);
        assert_eq!(bl, 2);
        assert_eq!(br, 3);
    }

    #[test]
    fn three_vertical_blocks_top_to_bottom() {
        let mut blocks = vec![
            block(0.0, 0.0,  100.0,  30.0),  // 底（0）
            block(0.0, 70.0, 100.0, 100.0),  // 顶（1）
            block(0.0, 35.0, 100.0,  65.0),  // 中（2）
        ];
        assign_reading_order(&mut blocks);
        assert_eq!(blocks[1].reading_order, 0, "顶部应 order=0");
        assert_eq!(blocks[2].reading_order, 1, "中部应 order=1");
        assert_eq!(blocks[0].reading_order, 2, "底部应 order=2");
    }

    #[test]
    fn empty_input_is_noop() {
        let mut blocks: Vec<TextBlock> = vec![];
        assign_reading_order(&mut blocks); // 不 panic
    }

    // ── 间隙探测 ─────────────────────────────────────────────────────────────

    #[test]
    fn h_cut_found_between_non_overlapping_blocks() {
        let blocks = vec![
            block(0.0, 50.0, 100.0, 100.0),
            block(0.0,  0.0, 100.0,  40.0),
        ];
        let cut = find_h_cut(&[0, 1], &blocks);
        assert!(cut.is_some(), "应找到水平切割线");
        let y = cut.unwrap();
        assert!(y > 40.0 && y < 50.0, "切割线应在间隙中 (40..50)，实际={y}");
    }

    #[test]
    fn h_cut_absent_for_overlapping_blocks() {
        // 两块在 y 方向重叠 → 无水平切割
        let blocks = vec![
            block(0.0, 30.0, 50.0, 100.0),
            block(60.0, 0.0, 110.0, 70.0),
        ];
        assert!(find_h_cut(&[0, 1], &blocks).is_none(), "重叠块不应有水平切割线");
    }

    #[test]
    fn v_cut_found_between_side_by_side_blocks() {
        let blocks = vec![
            block(0.0,  0.0,  50.0, 100.0),
            block(60.0, 0.0, 110.0, 100.0),
        ];
        let cut = find_v_cut(&[0, 1], &blocks);
        assert!(cut.is_some(), "应找到垂直切割线");
        let x = cut.unwrap();
        assert!(x > 50.0 && x < 60.0, "切割线应在间隙中 (50..60)，实际={x}");
    }

    // ── extract_text ─────────────────────────────────────────────────────────

    #[test]
    fn extract_text_respects_reading_order() {
        let mut blocks = vec![
            block_with_text(0.0, 0.0, 50.0, 40.0, "底部"),     // 下方
            block_with_text(0.0, 50.0, 50.0, 100.0, "顶部"),   // 上方
        ];
        assign_reading_order(&mut blocks);
        let text = extract_text(&blocks);
        // 顶部应先出现
        let top_pos    = text.find("顶部").unwrap();
        let bottom_pos = text.find("底部").unwrap();
        assert!(top_pos < bottom_pos, "顶部应在底部前出现\ntext={text:?}");
    }

    #[test]
    fn extract_text_joins_words_with_space() {
        let mut b = block_with_text(0.0, 0.0, 100.0, 50.0, "hello");
        // 手动添加第二个词
        let w2 = Word {
            glyphs: vec![],
            bounds: Rect::new(60.0, 0.0, 10.0, 12.0),
            text: "world".to_string(),
        };
        b.lines[0].words.push(w2);
        let mut blocks = vec![b];
        assign_reading_order(&mut blocks);
        let text = extract_text(&blocks);
        assert_eq!(text.trim(), "hello world");
    }
}
