use lopdf::{content::Content, Object};
use vellum_pdf::{Matrix, Point, Rect};
use crate::glyph::{MappingMethod, RawGlyph};

/// PDF 内容流文本解释器：解析 BT…ET 块，提取原始字形列表。
///
/// Phase 2 产物：字形 ID 直接来自内容流字节，Unicode 映射留待 Phase 3。
/// 字形宽度用 600/1000 单位宽度近似，Phase 3 读取真实字宽数组后替换。
pub struct StreamInterpreter;

impl StreamInterpreter {
    pub fn new() -> Self { Self }

    /// 解析内容流字节，返回原始字形列表（`RawGlyph::unicode` 此阶段均为 `None`）。
    pub fn run(&self, bytes: &[u8]) -> Vec<RawGlyph> {
        let ops = match Content::decode(bytes) {
            Ok(c)  => c.operations,
            Err(_) => return vec![],
        };
        let mut state = State::new();
        for op in &ops {
            state.apply(&op.operator, &op.operands);
        }
        state.glyphs
    }
}

impl Default for StreamInterpreter {
    fn default() -> Self { Self }
}

// ── 状态机 ────────────────────────────────────────────────────────────────────

struct State {
    /// 图形状态栈（q/Q）
    gs_stack: Vec<SavedGraphics>,
    ctm: Matrix,
    text: TextState,
    in_text: bool,
    glyphs: Vec<RawGlyph>,
}

/// q/Q 保存的图形状态（仅 CTM；文本矩阵不在图形状态内，不随 q/Q 还原）
struct SavedGraphics {
    ctm: Matrix,
}

#[derive(Clone)]
struct TextState {
    font_name:    String,
    font_size:    f32,
    char_spacing: f32,
    word_spacing: f32,
    /// 水平缩放比（%），默认 100
    h_scaling:    f32,
    leading:      f32,
    rise:         f32,
    /// 文本矩阵 Tm：记录当前字形坐标原点
    text_matrix:  Matrix,
    /// 行矩阵 Tlm：Td / TD / T* 的基准
    line_matrix:  Matrix,
}

impl TextState {
    fn new() -> Self {
        Self {
            font_name:    String::new(),
            font_size:    0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scaling:    100.0,
            leading:      0.0,
            rise:         0.0,
            text_matrix:  Matrix::identity(),
            line_matrix:  Matrix::identity(),
        }
    }
}

impl State {
    fn new() -> Self {
        Self {
            gs_stack: vec![],
            ctm:      Matrix::identity(),
            text:     TextState::new(),
            in_text:  false,
            glyphs:   vec![],
        }
    }

    fn apply(&mut self, op: &str, o: &[Object]) {
        match op {
            // ── 图形状态栈 ──────────────────────────────────────────────────
            "q" => self.gs_stack.push(SavedGraphics { ctm: self.ctm }),
            "Q" => {
                if let Some(saved) = self.gs_stack.pop() {
                    self.ctm = saved.ctm;
                }
            }
            "cm" if o.len() >= 6 => {
                self.ctm = self.ctm.concat(&mat6(o));
            }

            // ── 文本对象边界 ────────────────────────────────────────────────
            "BT" => {
                self.in_text           = true;
                self.text.text_matrix  = Matrix::identity();
                self.text.line_matrix  = Matrix::identity();
            }
            "ET" => { self.in_text = false; }

            // ── 字体与文本状态 ──────────────────────────────────────────────
            "Tf" if o.len() >= 2 => {
                if let Object::Name(n) = &o[0] {
                    self.text.font_name = format!("/{}", String::from_utf8_lossy(n));
                }
                self.text.font_size = real(&o[1]);
            }
            "Tc" if !o.is_empty() => { self.text.char_spacing  = real(&o[0]); }
            "Tw" if !o.is_empty() => { self.text.word_spacing   = real(&o[0]); }
            "Tz" if !o.is_empty() => { self.text.h_scaling      = real(&o[0]); }
            "TL" if !o.is_empty() => { self.text.leading        = real(&o[0]); }
            "Ts" if !o.is_empty() => { self.text.rise           = real(&o[0]); }

            // ── 文本位置 ────────────────────────────────────────────────────
            "Td" | "TD" if o.len() >= 2 => {
                let (tx, ty) = (real(&o[0]), real(&o[1]));
                if op == "TD" { self.text.leading = -ty; }
                let tr = Matrix([1.0, 0.0, 0.0, 1.0, tx, ty]);
                self.text.line_matrix = self.text.line_matrix.concat(&tr);
                self.text.text_matrix = self.text.line_matrix;
            }
            "Tm" if o.len() >= 6 => {
                let m = mat6(o);
                self.text.text_matrix = m;
                self.text.line_matrix = m;
            }
            "T*" => {
                // 等价于 0 -TL Td
                let tr = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, -self.text.leading]);
                self.text.line_matrix = self.text.line_matrix.concat(&tr);
                self.text.text_matrix = self.text.line_matrix;
            }

            // ── 文本显示 ────────────────────────────────────────────────────
            "Tj" if self.in_text && !o.is_empty() => {
                if let Object::String(b, _) = &o[0] {
                    let bytes = b.clone();
                    self.emit_string(&bytes);
                }
            }
            "TJ" if self.in_text && !o.is_empty() => {
                if let Object::Array(arr) = &o[0] {
                    let arr = arr.clone();
                    for item in &arr {
                        match item {
                            Object::String(b, _) => { let bb = b.clone(); self.emit_string(&bb); }
                            Object::Integer(k)   => self.apply_kern(*k as f32),
                            Object::Real(k)      => self.apply_kern(*k as f32),
                            _ => {}
                        }
                    }
                }
            }
            // T_newline (') ：先换行，再显示字符串
            "'" if self.in_text && !o.is_empty() => {
                let tr = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, -self.text.leading]);
                self.text.line_matrix = self.text.line_matrix.concat(&tr);
                self.text.text_matrix = self.text.line_matrix;
                if let Object::String(b, _) = &o[0] {
                    let bb = b.clone(); self.emit_string(&bb);
                }
            }
            // T_w_space (") ：设置 Tw/Tc，换行，再显示字符串
            "\"" if self.in_text && o.len() >= 3 => {
                self.text.word_spacing = real(&o[0]);
                self.text.char_spacing = real(&o[1]);
                let tr = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, -self.text.leading]);
                self.text.line_matrix = self.text.line_matrix.concat(&tr);
                self.text.text_matrix = self.text.line_matrix;
                if let Object::String(b, _) = &o[2] {
                    let bb = b.clone(); self.emit_string(&bb);
                }
            }

            _ => {}
        }
    }

    /// 将字节串中的每个字节作为一个字形 ID 发射，并推进文本矩阵。
    fn emit_string(&mut self, bytes: &[u8]) {
        let fs = self.text.font_size;
        let th = self.text.h_scaling / 100.0;
        // 近似字宽：600/1000 单位（Latin 平均值）；Phase 3 改用真实字宽数组
        const NOMINAL_W: f32 = 600.0;

        for &byte in bytes {
            let origin  = self.glyph_origin();
            let advance = (NOMINAL_W / 1000.0 * fs + self.text.char_spacing) * th;
            let w_sp    = if byte == b' ' { self.text.word_spacing * th } else { 0.0 };
            let total   = advance + w_sp;

            self.glyphs.push(RawGlyph {
                glyph_id:       byte as u32,
                unicode:        None,
                mapping_method: MappingMethod::Unmapped,
                origin,
                bounds: Rect::new(origin.x, origin.y, total.max(0.0), fs.abs()),
                font_size:      fs,
                font_name:      self.text.font_name.clone(),
            });

            // 沿文本行方向平移 Tm（PDF row-vector 语义：new_Tm = translate × Tm）
            // 对应 col-vector：Tm.concat(translate)
            let tr = Matrix([1.0, 0.0, 0.0, 1.0, total, 0.0]);
            self.text.text_matrix = self.text.text_matrix.concat(&tr);
        }
    }

    /// TJ kern 值：单位为 1/1000 字号，负值向右（正 x）方向移动
    fn apply_kern(&mut self, kern: f32) {
        let tx = -kern / 1000.0 * self.text.font_size * (self.text.h_scaling / 100.0);
        let tr = Matrix([1.0, 0.0, 0.0, 1.0, tx, 0.0]);
        self.text.text_matrix = self.text.text_matrix.concat(&tr);
    }

    /// 当前字形原点（页面坐标）。
    ///
    /// 文本渲染矩阵 Trm = [fs·Th  0  0  fs  0  rise] × Tm × CTM（PDF row-vector 写法）。
    /// 在 col-vector 表示下：CTM.concat(Tm.concat(scale))。
    /// 对 (0,0) 应用即得字形原点。
    fn glyph_origin(&self) -> Point {
        let fs   = self.text.font_size;
        let th   = self.text.h_scaling / 100.0;
        let rise = self.text.rise;
        let scale = Matrix([fs * th, 0.0, 0.0, fs, 0.0, rise]);
        let trm   = self.ctm.concat(&self.text.text_matrix.concat(&scale));
        trm.transform_point(Point { x: 0.0, y: 0.0 })
    }
}

// ── 小工具 ────────────────────────────────────────────────────────────────────

fn mat6(o: &[Object]) -> Matrix {
    Matrix([real(&o[0]), real(&o[1]), real(&o[2]), real(&o[3]), real(&o[4]), real(&o[5])])
}

fn real(obj: &Object) -> f32 {
    match obj {
        Object::Real(f)    => *f as f32,
        Object::Integer(n) => *n as f32,
        _ => 0.0,
    }
}

// ── 单元测试 ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &[u8]) -> Vec<RawGlyph> {
        StreamInterpreter::new().run(s)
    }

    #[test]
    fn tm_sets_glyph_origin() {
        let g = run(b"BT\n/F1 12 Tf\n1 0 0 1 100 200 Tm\n(A) Tj\nET");
        assert_eq!(g.len(), 1);
        assert!((g[0].origin.x - 100.0).abs() < 0.01, "x={}", g[0].origin.x);
        assert!((g[0].origin.y - 200.0).abs() < 0.01, "y={}", g[0].origin.y);
        assert_eq!(g[0].font_size, 12.0);
        assert_eq!(&g[0].font_name, "/F1");
        assert_eq!(g[0].glyph_id, b'A' as u32);
    }

    #[test]
    fn tm_y_flip_preserves_origin() {
        // y-flip 矩阵：a=1 b=0 c=0 d=-1 → 坐标原点仍在 (100, 200)
        let g = run(b"BT\n/F1 12 Tf\n1 0 0 -1 100 200 Tm\n(X) Tj\nET");
        assert_eq!(g.len(), 1);
        assert!((g[0].origin.x - 100.0).abs() < 0.01);
        assert!((g[0].origin.y - 200.0).abs() < 0.01);
    }

    #[test]
    fn td_translates_from_origin() {
        let g = run(b"BT\n/F1 12 Tf\n72 720 Td\n(H) Tj\nET");
        assert_eq!(g.len(), 1);
        assert!((g[0].origin.x - 72.0).abs() < 0.01);
        assert!((g[0].origin.y - 720.0).abs() < 0.01);
    }

    #[test]
    fn multiple_glyphs_advance_along_x() {
        let g = run(b"BT\n/F1 12 Tf\n0 0 Td\n(AB) Tj\nET");
        assert_eq!(g.len(), 2);
        assert!(g[1].origin.x > g[0].origin.x, "B 应在 A 的右侧");
        assert_eq!(g[0].origin.y, g[1].origin.y);
    }

    #[test]
    fn tj_negative_kern_moves_right() {
        let g_no  = run(b"BT\n/F1 12 Tf\n0 0 Td\n[(A)(B)] TJ\nET");
        let g_neg = run(b"BT\n/F1 12 Tf\n0 0 Td\n[(A) -500 (B)] TJ\nET");
        assert_eq!(g_no.len(), 2);
        assert_eq!(g_neg.len(), 2);
        assert!(
            g_neg[1].origin.x > g_no[1].origin.x,
            "kern=-500 应使 B 右移: no={} neg={}", g_no[1].origin.x, g_neg[1].origin.x
        );
    }

    #[test]
    fn t_star_advances_line() {
        // TL=20，T* 后 y 偏移 -20
        let g = run(b"BT\n/F1 12 Tf\n20 TL\n0 700 Td\n(A) Tj\nT*\n(B) Tj\nET");
        assert_eq!(g.len(), 2);
        let dy = g[1].origin.y - g[0].origin.y;
        assert!((dy - (-20.0)).abs() < 0.01, "T* 应使 y 减少 TL=20, 实际 dy={dy}");
    }

    #[test]
    fn outside_bt_et_no_glyphs() {
        let g = run(b"/F1 12 Tf\n72 720 Td\n(Hello) Tj");
        assert!(g.is_empty(), "BT…ET 外部操作不应产生字形");
    }

    #[test]
    fn empty_stream_ok() {
        assert!(run(b"").is_empty());
    }

    #[test]
    fn q_q_restores_ctm() {
        // cm 移到 (50, 50)，q 保存，cm 再移 (100, 100)，Q 还原，再显示字形
        // 期望字形原点 ≈ (50, 50)
        let g = run(
            b"1 0 0 1 50 50 cm\nq\n1 0 0 1 100 100 cm\nQ\nBT\n/F1 12 Tf\n0 0 Td\n(A) Tj\nET"
        );
        assert_eq!(g.len(), 1);
        assert!((g[0].origin.x - 50.0).abs() < 0.01, "Q 应还原 CTM, x={}", g[0].origin.x);
        assert!((g[0].origin.y - 50.0).abs() < 0.01, "Q 应还原 CTM, y={}", g[0].origin.y);
    }

    // ── 位置操作符 ────────────────────────────────────────────────────────────

    #[test]
    fn td_accumulates() {
        // 两次 Td 叠加：(10,0) + (5,0) → (15, 0)
        let g = run(b"BT\n/F1 12 Tf\n10 0 Td\n5 0 Td\n(A) Tj\nET");
        assert_eq!(g.len(), 1);
        assert!((g[0].origin.x - 15.0).abs() < 0.01, "x={}", g[0].origin.x);
        assert!((g[0].origin.y - 0.0).abs() < 0.01,  "y={}", g[0].origin.y);
    }

    #[test]
    fn td_large_uppercase_updates_leading() {
        // TD 等同 Td 并设置 TL = -ty，后续 T* 应沿用这个行距
        // 0 -20 TD → TL=20，T* 再移动 -20
        let g = run(b"BT\n/F1 12 Tf\n0 100 Td\n0 -20 TD\n(A) Tj\nT*\n(B) Tj\nET");
        assert_eq!(g.len(), 2);
        assert!((g[0].origin.y - 80.0).abs() < 0.01, "A.y={}", g[0].origin.y);
        assert!((g[1].origin.y - 60.0).abs() < 0.01, "B.y={}", g[1].origin.y);
    }

    // ── 文本状态操作符 ────────────────────────────────────────────────────────

    #[test]
    fn tc_widens_glyph_advance() {
        // Tc=5 → advance = (0.6*12 + 5)*1 = 12.2；比默认 7.2 宽 5 个单位
        let g_no = run(b"BT\n/F1 12 Tf\n0 0 Td\n(AB) Tj\nET");
        let g_tc = run(b"BT\n/F1 12 Tf\n5 Tc\n0 0 Td\n(AB) Tj\nET");
        assert_eq!(g_no.len(), 2);
        assert_eq!(g_tc.len(), 2);
        let diff = g_tc[1].origin.x - g_no[1].origin.x;
        assert!((diff - 5.0).abs() < 0.01, "Tc=5 应使第二字形右移 5，实际差={diff}");
    }

    #[test]
    fn tw_widens_space_character_only() {
        // Tw=10 只对空格字节（0x20）额外增加 10 个单位
        // 内容：A(0x41) space(0x20) B(0x42)
        let g_no = run(b"BT\n/F1 12 Tf\n0 0 Td\n(A B) Tj\nET");
        let g_tw = run(b"BT\n/F1 12 Tf\n10 Tw\n0 0 Td\n(A B) Tj\nET");
        assert_eq!(g_no.len(), 3);
        assert_eq!(g_tw.len(), 3);
        // A 的位置不变
        assert_eq!(g_no[0].origin.x, g_tw[0].origin.x, "Tw 不影响非空格字形起点");
        // 空格后 B 的位置差 = Tw = 10
        let diff = g_tw[2].origin.x - g_no[2].origin.x;
        assert!((diff - 10.0).abs() < 0.01, "Tw=10 应使 B 右移 10，实际差={diff}");
    }

    #[test]
    fn tz_scales_horizontal_advance() {
        // Tz=50 → advance 减半；两个字形的间距比 Tz=100 时缩短一半
        let g_100 = run(b"BT\n/F1 12 Tf\n0 0 Td\n(AB) Tj\nET");
        let g_50  = run(b"BT\n/F1 12 Tf\n50 Tz\n0 0 Td\n(AB) Tj\nET");
        assert_eq!(g_100.len(), 2);
        assert_eq!(g_50.len(), 2);
        let dx_100 = g_100[1].origin.x - g_100[0].origin.x;
        let dx_50  = g_50[1].origin.x  - g_50[0].origin.x;
        assert!((dx_50 - dx_100 / 2.0).abs() < 0.01,
            "Tz=50 应使间距减半：dx_100={dx_100} dx_50={dx_50}");
    }

    #[test]
    fn ts_shifts_origin_y_by_rise() {
        // Ts=5 → 文本渲染矩阵的 rise 分量，字形原点 y 偏移 +5
        let g_0 = run(b"BT\n/F1 12 Tf\n0 0 Td\n(A) Tj\nET");
        let g_5 = run(b"BT\n/F1 12 Tf\n5 Ts\n0 0 Td\n(A) Tj\nET");
        assert_eq!(g_0.len(), 1);
        assert_eq!(g_5.len(), 1);
        let diff = g_5[0].origin.y - g_0[0].origin.y;
        assert!((diff - 5.0).abs() < 0.01, "Ts=5 应使 y 增加 5，实际差={diff}");
    }

    // ── cm 与 CTM ─────────────────────────────────────────────────────────────

    #[test]
    fn cm_shifts_glyph_positions() {
        // cm 将坐标系平移 (80, 40)，BT 内的 0 0 Td 字形原点应在 (80, 40)
        let g = run(b"1 0 0 1 80 40 cm\nBT\n/F1 12 Tf\n0 0 Td\n(A) Tj\nET");
        assert_eq!(g.len(), 1);
        assert!((g[0].origin.x - 80.0).abs() < 0.01, "x={}", g[0].origin.x);
        assert!((g[0].origin.y - 40.0).abs() < 0.01, "y={}", g[0].origin.y);
    }

    // ── BT/ET 语义 ────────────────────────────────────────────────────────────

    #[test]
    fn bt_resets_text_matrix() {
        // 第一个 BT 块设置 Tm，第二个 BT 应重置为 identity
        // 第二块无 Td/Tm → 字形在 (0, 0)（需要保持字号）
        let g = run(b"BT\n/F1 12 Tf\n1 0 0 1 200 300 Tm\n(A) Tj\nET\nBT\n(B) Tj\nET");
        assert_eq!(g.len(), 2);
        assert!((g[1].origin.x - 0.0).abs() < 0.01, "第二个 BT 应重置 Tm, x={}", g[1].origin.x);
        assert!((g[1].origin.y - 0.0).abs() < 0.01, "第二个 BT 应重置 Tm, y={}", g[1].origin.y);
    }

    #[test]
    fn multiple_bt_et_blocks_independent() {
        let g = run(b"BT\n/F1 12 Tf\n50 100 Td\n(A) Tj\nET\nBT\n/F1 12 Tf\n200 300 Td\n(B) Tj\nET");
        assert_eq!(g.len(), 2);
        assert!((g[0].origin.x - 50.0).abs()  < 0.01);
        assert!((g[0].origin.y - 100.0).abs() < 0.01);
        assert!((g[1].origin.x - 200.0).abs() < 0.01);
        assert!((g[1].origin.y - 300.0).abs() < 0.01);
    }

    // ── 文本显示操作符 ────────────────────────────────────────────────────────

    #[test]
    fn tj_positive_kern_moves_left() {
        // kern=+500 → tx = -500/1000*12 = -6，B 应比无 kern 时更靠左
        let g_no  = run(b"BT\n/F1 12 Tf\n0 0 Td\n[(A)(B)] TJ\nET");
        let g_pos = run(b"BT\n/F1 12 Tf\n0 0 Td\n[(A) 500 (B)] TJ\nET");
        assert!(g_pos[1].origin.x < g_no[1].origin.x,
            "kern=+500 应使 B 左移: no={} pos={}", g_no[1].origin.x, g_pos[1].origin.x);
    }

    #[test]
    fn tj_strings_only_concatenate() {
        // TJ 数组只含字符串时等同于依次 Tj，共 5 个字形
        let g = run(b"BT\n/F1 12 Tf\n0 0 Td\n[(AB)(CDE)] TJ\nET");
        assert_eq!(g.len(), 5, "字形数={}", g.len());
        // 位置单调递增
        for i in 1..g.len() {
            assert!(g[i].origin.x > g[i-1].origin.x);
        }
    }

    #[test]
    fn apostrophe_moves_to_next_line_and_emits() {
        // ' 等同于 T* + Tj：先换行（y -= TL）再显示字符串
        let g = run(b"BT\n/F1 12 Tf\n15 TL\n0 200 Td\n(A) Tj\n(B) '\nET");
        assert_eq!(g.len(), 2);
        assert!((g[0].origin.y - 200.0).abs() < 0.01, "A.y={}", g[0].origin.y);
        // B 在新行：y = 200 - 15 = 185，x 回到行首（line_matrix.e = 0）
        assert!((g[1].origin.y - 185.0).abs() < 0.01, "B.y={}", g[1].origin.y);
        assert!((g[1].origin.x - 0.0).abs()   < 0.01, "B.x={}", g[1].origin.x);
    }

    #[test]
    fn quote_sets_tw_tc_and_emits() {
        // " 设置 Tw、Tc，然后换行并显示字符串
        // 验证：设置后的 Tc 影响后续 Tj 的字形间距
        let stream = b"BT\n/F1 12 Tf\n10 TL\n0 200 Td\n(A) Tj\n0 8 (B) \"\n(CD) Tj\nET";
        let g = run(stream);
        // A：1 个，B：1 个（来自 "），C/D：2 个（来自后续 Tj，Tc=8 生效）
        assert_eq!(g.len(), 4);
        // B 在新行 y=190（200-10），x=0
        assert!((g[1].origin.y - 190.0).abs() < 0.01, "B.y={}", g[1].origin.y);
        // C/D 之间距离 = (0.6*12+8)*1 = 15.2，比无 Tc 时 (7.2) 宽
        let dx = g[3].origin.x - g[2].origin.x;
        assert!((dx - 15.2).abs() < 0.1, "Tc=8 生效后 CD 间距应 ≈15.2, 实际={dx}");
    }

    // ── 字形字段正确性 ────────────────────────────────────────────────────────

    #[test]
    fn glyph_id_equals_byte_value() {
        // 任意字节值应原样作为 glyph_id，不做任何映射
        let g = run(b"BT\n/F1 12 Tf\n0 0 Td\n(\xFF\x00\x41) Tj\nET");
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].glyph_id, 0xFF);
        assert_eq!(g[1].glyph_id, 0x00);
        assert_eq!(g[2].glyph_id, 0x41);
    }

    #[test]
    fn phase2_unicode_is_none_and_unmapped() {
        use crate::glyph::MappingMethod;
        let g = run(b"BT\n/F1 12 Tf\n0 0 Td\n(ABC) Tj\nET");
        assert!(!g.is_empty());
        for glyph in &g {
            assert!(glyph.unicode.is_none(),        "Phase 2 unicode 应为 None");
            assert_eq!(glyph.mapping_method, MappingMethod::Unmapped, "Phase 2 应为 Unmapped");
        }
    }

    #[test]
    fn bounds_width_equals_advance() {
        // 无 Tc/Tw/Tz，advance = 0.6 * 12 = 7.2，bounds 宽度应相同
        let g = run(b"BT\n/F1 12 Tf\n0 0 Td\n(A) Tj\nET");
        assert_eq!(g.len(), 1);
        let w = g[0].bounds.max.x - g[0].bounds.min.x;
        assert!((w - 7.2).abs() < 0.01, "bounds 宽度={w}, 期望 ≈7.2");
        assert!((g[0].bounds.height() - 12.0).abs() < 0.01, "bounds 高度应等于字号 12");
    }
}
