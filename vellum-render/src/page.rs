use lopdf::content::{Content, Operation};
use lopdf::Object;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
use vellum_pdf::{Font, Matrix, PageFonts, Point};

// ── 公共接口 ──────────────────────────────────────────────────────────────────

/// 将单页内容流光栅化为位图。
///
/// 支持：图形状态栈、路径构造与填充/描边、DeviceGray/RGB/CMYK 颜色、
/// Type3 字体（递归执行 CharProc）、嵌入 TrueType 字体（FontFile2）。
/// 暂不支持：裁剪路径、图像 XObject、渐变、ExtGState 透明度。
pub fn render_page(
    page_w: f32,
    page_h: f32,
    content: &[u8],
    fonts: &PageFonts,
    dpi: f32,
) -> Option<Pixmap> {
    let pw = (page_w * dpi).round() as u32;
    let ph = (page_h * dpi).round() as u32;
    if pw == 0 || ph == 0 { return None; }

    let mut pixmap = Pixmap::new(pw, ph)?;
    pixmap.fill(Color::WHITE);

    let ops = Content::decode(content).ok()?.operations;

    // 初始 CTM：PDF 用户空间（左下原点，y 向上）→ 设备像素（左上原点，y 向下）
    let base_ctm = Matrix([dpi, 0.0, 0.0, -dpi, 0.0, page_h * dpi]);

    let mut interp = Interp::new(&mut pixmap);
    interp.gstack.push(GState::new(base_ctm));
    interp.exec_ops(&ops, fonts);

    Some(pixmap)
}

// ── 图形状态 ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct GState {
    ctm:        Matrix,
    fill:       Color,
    stroke:     Color,
    line_width: f32,
}

impl GState {
    fn new(ctm: Matrix) -> Self {
        Self { ctm, fill: Color::BLACK, stroke: Color::BLACK, line_width: 1.0 }
    }
}

#[derive(Clone)]
struct TextState {
    font_key:  Option<String>,
    font_size: f32,
    char_sp:   f32,
    word_sp:   f32,
    h_scale:   f32,
    leading:   f32,
    rise:      f32,
    tm:  Matrix,
    tlm: Matrix,
}

impl TextState {
    fn new() -> Self {
        Self {
            font_key: None, font_size: 0.0,
            char_sp: 0.0, word_sp: 0.0, h_scale: 100.0, leading: 0.0, rise: 0.0,
            tm: Matrix::identity(), tlm: Matrix::identity(),
        }
    }
}

// ── 路径段（用户空间）──────────────────────────────────────────────────────────

enum Seg {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

// ── 解释器 ────────────────────────────────────────────────────────────────────

struct Interp<'a> {
    pixmap: &'a mut Pixmap,
    gstack: Vec<GState>,
    path:   Vec<Seg>,
    cur:    Point,
    start:  Point,
    text:   TextState,
}

impl<'a> Interp<'a> {
    fn new(pixmap: &'a mut Pixmap) -> Self {
        Self {
            pixmap,
            gstack: vec![],
            path:   vec![],
            cur:    Point { x: 0.0, y: 0.0 },
            start:  Point { x: 0.0, y: 0.0 },
            text:   TextState::new(),
        }
    }

    fn gs(&self) -> &GState { self.gstack.last().unwrap() }
    fn gs_mut(&mut self) -> &mut GState { self.gstack.last_mut().unwrap() }

    fn exec_ops(&mut self, ops: &[Operation], fonts: &PageFonts) {
        for op in ops {
            self.exec(op, fonts);
        }
    }

    fn exec(&mut self, op: &Operation, fonts: &PageFonts) {
        let o = &op.operands;
        match op.operator.as_str() {
            // ── 图形状态 ────────────────────────────────────────────────────
            "q" => { let g = self.gs().clone(); self.gstack.push(g); }
            "Q" => { if self.gstack.len() > 1 { self.gstack.pop(); } }
            "cm" if o.len() >= 6 => {
                let m = mat6(o);
                let ctm = self.gs().ctm.concat(&m);
                self.gs_mut().ctm = ctm;
            }
            "w" if !o.is_empty() => { self.gs_mut().line_width = f(&o[0]); }
            "gs" => {} // ExtGState：忽略

            // ── 颜色 ────────────────────────────────────────────────────────
            "g"  if !o.is_empty()   => { let v = f(&o[0]); self.gs_mut().fill   = gray(v); }
            "G"  if !o.is_empty()   => { let v = f(&o[0]); self.gs_mut().stroke = gray(v); }
            "rg" if o.len() >= 3    => { self.gs_mut().fill   = rgb(o); }
            "RG" if o.len() >= 3    => { self.gs_mut().stroke = rgb(o); }
            "k"  if o.len() >= 4    => { self.gs_mut().fill   = cmyk(o); }
            "K"  if o.len() >= 4    => { self.gs_mut().stroke = cmyk(o); }
            "sc" | "scn"            => { if let Some(c) = scn(o) { self.gs_mut().fill   = c; } }
            "SC" | "SCN"            => { if let Some(c) = scn(o) { self.gs_mut().stroke = c; } }
            "cs" | "CS" => {} // 颜色空间：忽略，靠操作数个数推断

            // ── 路径构造 ────────────────────────────────────────────────────
            "m" if o.len() >= 2 => {
                let p = pt(&o[0], &o[1]); self.path.push(Seg::Move(p));
                self.cur = p; self.start = p;
            }
            "l" if o.len() >= 2 => {
                let p = pt(&o[0], &o[1]); self.path.push(Seg::Line(p)); self.cur = p;
            }
            "c" if o.len() >= 6 => {
                let (c1, c2, e) = (pt(&o[0],&o[1]), pt(&o[2],&o[3]), pt(&o[4],&o[5]));
                self.path.push(Seg::Cubic(c1, c2, e)); self.cur = e;
            }
            "v" if o.len() >= 4 => {
                let (c2, e) = (pt(&o[0],&o[1]), pt(&o[2],&o[3]));
                self.path.push(Seg::Cubic(self.cur, c2, e)); self.cur = e;
            }
            "y" if o.len() >= 4 => {
                let (c1, e) = (pt(&o[0],&o[1]), pt(&o[2],&o[3]));
                self.path.push(Seg::Cubic(c1, e, e)); self.cur = e;
            }
            "re" if o.len() >= 4 => {
                let (x, y, w, h) = (f(&o[0]), f(&o[1]), f(&o[2]), f(&o[3]));
                self.path.push(Seg::Move(Point { x, y }));
                self.path.push(Seg::Line(Point { x: x+w, y }));
                self.path.push(Seg::Line(Point { x: x+w, y: y+h }));
                self.path.push(Seg::Line(Point { x, y: y+h }));
                self.path.push(Seg::Close);
                self.start = Point { x, y }; self.cur = self.start;
            }
            "h" => { self.path.push(Seg::Close); self.cur = self.start; }

            // ── 路径绘制 ────────────────────────────────────────────────────
            "f" | "F" => self.paint(true, false, FillRule::Winding),
            "f*"      => self.paint(true, false, FillRule::EvenOdd),
            "S"       => self.paint(false, true, FillRule::Winding),
            "s"       => { self.path.push(Seg::Close); self.paint(false, true, FillRule::Winding); }
            "B" | "b" => {
                if op.operator == "b" { self.path.push(Seg::Close); }
                self.paint(true, true, FillRule::Winding);
            }
            "B*" | "b*" => {
                if op.operator == "b*" { self.path.push(Seg::Close); }
                self.paint(true, true, FillRule::EvenOdd);
            }
            "n" => self.path.clear(),
            "W" | "W*" => {} // 裁剪：忽略（路径在随后的 n/f 处清空）

            // ── 文本 ────────────────────────────────────────────────────────
            "BT" => { self.text.tm = Matrix::identity(); self.text.tlm = Matrix::identity(); }
            "ET" => {}
            "Tf" if o.len() >= 2 => {
                if let Object::Name(n) = &o[0] {
                    self.text.font_key = Some(format!("/{}", String::from_utf8_lossy(n)));
                }
                self.text.font_size = f(&o[1]);
            }
            "Tc" if !o.is_empty() => self.text.char_sp = f(&o[0]),
            "Tw" if !o.is_empty() => self.text.word_sp = f(&o[0]),
            "Tz" if !o.is_empty() => self.text.h_scale = f(&o[0]),
            "TL" if !o.is_empty() => self.text.leading = f(&o[0]),
            "Ts" if !o.is_empty() => self.text.rise = f(&o[0]),
            "Td" | "TD" if o.len() >= 2 => {
                let (tx, ty) = (f(&o[0]), f(&o[1]));
                if op.operator == "TD" { self.text.leading = -ty; }
                let tr = Matrix([1.0,0.0,0.0,1.0,tx,ty]);
                self.text.tlm = self.text.tlm.concat(&tr);
                self.text.tm  = self.text.tlm;
            }
            "Tm" if o.len() >= 6 => { let m = mat6(o); self.text.tm = m; self.text.tlm = m; }
            "T*" => {
                let tr = Matrix([1.0,0.0,0.0,1.0,0.0,-self.text.leading]);
                self.text.tlm = self.text.tlm.concat(&tr);
                self.text.tm  = self.text.tlm;
            }
            "Tj" if !o.is_empty() => {
                if let Object::String(b, _) = &o[0] { self.show(b.clone(), fonts); }
            }
            "TJ" if !o.is_empty() => {
                if let Object::Array(arr) = &o[0] {
                    for item in arr.clone() {
                        match item {
                            Object::String(b, _) => self.show(b, fonts),
                            Object::Integer(k)   => self.kern(k as f32),
                            Object::Real(k)      => self.kern(k),
                            _ => {}
                        }
                    }
                }
            }
            "'" if !o.is_empty() => {
                let tr = Matrix([1.0,0.0,0.0,1.0,0.0,-self.text.leading]);
                self.text.tlm = self.text.tlm.concat(&tr);
                self.text.tm  = self.text.tlm;
                if let Object::String(b, _) = &o[0] { self.show(b.clone(), fonts); }
            }

            // ── d0/d1：Type3 字形度量，仅消费 ────────────────────────────────
            "d0" | "d1" => {}

            _ => {}
        }
    }

    // ── 填充/描边当前路径 ─────────────────────────────────────────────────────

    fn paint(&mut self, fill: bool, stroke: bool, rule: FillRule) {
        let ctm = self.gs().ctm;
        if let Some(path) = build_path(&self.path, &ctm) {
            if fill {
                let mut paint = Paint::default();
                paint.set_color(self.gs().fill);
                paint.anti_alias = true;
                self.pixmap.fill_path(&path, &paint, rule, Transform::identity(), None);
            }
            if stroke {
                let mut paint = Paint::default();
                paint.set_color(self.gs().stroke);
                paint.anti_alias = true;
                let w = (self.gs().line_width * ctm_scale(&ctm)).max(0.4);
                let s = Stroke { width: w, ..Default::default() };
                self.pixmap.stroke_path(&path, &paint, &s, Transform::identity(), None);
            }
        }
        self.path.clear();
    }

    // ── 显示文本字符串 ────────────────────────────────────────────────────────

    fn show(&mut self, bytes: Vec<u8>, fonts: &PageFonts) {
        let Some(key) = self.text.font_key.clone() else { return };
        let Some(font) = fonts.get(&key) else {
            // 未知字体：仍按单字节推进，避免后续字形错位
            self.advance_default(bytes.len());
            return;
        };

        let two_byte = matches!(font, Font::TrueType { two_byte: true, .. });
        let codes = decode_codes(&bytes, two_byte);

        for code in codes {
            match font {
                Font::Type3 { font_matrix, char_procs, widths } => {
                    if let Some(proc_bytes) = char_procs.get(&(code as u8)) {
                        self.draw_type3(proc_bytes, font_matrix, fonts);
                    }
                    let w = widths.get(&(code as u8)).copied().unwrap_or(0.0);
                    // 字形宽度（字形空间）× FontMatrix 水平分量 → 文本空间
                    self.advance(w * font_matrix[0]);
                }
                Font::TrueType { program, .. } => {
                    self.draw_truetype(program, code);
                    self.advance(0.5); // 缺字宽数据时的近似推进（多数 PDF 用 Tm 重定位）
                }
                Font::Unsupported => self.advance(0.5),
            }
        }
    }

    /// 递归执行 Type3 CharProc：以字形渲染矩阵为 CTM，继承当前填充色。
    fn draw_type3(&mut self, proc_bytes: &[u8], font_matrix: &[f32; 6], fonts: &PageFonts) {
        let Ok(content) = Content::decode(proc_bytes) else { return };
        let glyph_ctm = self.glyph_matrix().concat(&Matrix(*font_matrix));

        // 保存当前路径/图形栈，建立字形专用图形状态
        let saved_path = std::mem::take(&mut self.path);
        let fill = self.gs().fill;
        self.gstack.push(GState { ctm: glyph_ctm, fill, stroke: fill, line_width: 1.0 });

        self.exec_ops(&content.operations, fonts);

        self.gstack.pop();
        self.path = saved_path;
    }

    /// 绘制 TrueType 字形轮廓（gid 直接取自编码，Identity CIDToGIDMap）。
    fn draw_truetype(&mut self, program: &[u8], gid: u32) {
        let Ok(face) = ttf_parser::Face::parse(program, 0) else { return };
        let upem = face.units_per_em() as f32;
        if upem <= 0.0 { return }

        let scale = Matrix([1.0/upem, 0.0, 0.0, 1.0/upem, 0.0, 0.0]);
        let m = self.glyph_matrix().concat(&scale);

        let mut builder = OutlineToPath { segs: vec![], m, cur: Point { x: 0.0, y: 0.0 } };
        if face.outline_glyph(ttf_parser::GlyphId(gid as u16), &mut builder).is_none() {
            return;
        }
        let Some(path) = builder.finish() else { return };
        let mut paint = Paint::default();
        paint.set_color(self.gs().fill);
        paint.anti_alias = true;
        self.pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }

    /// 当前字形渲染矩阵（不含 FontMatrix/upem 缩放）：CTM ∘ Tm ∘ 文本缩放。
    fn glyph_matrix(&self) -> Matrix {
        let fs = self.text.font_size;
        let th = self.text.h_scale / 100.0;
        let scale = Matrix([fs*th, 0.0, 0.0, fs, 0.0, self.text.rise]);
        self.gs().ctm.concat(&self.text.tm).concat(&scale)
    }

    /// 沿文本行方向推进文本矩阵（tx 为文本空间位移，不含字号缩放）。
    fn advance(&mut self, w_text: f32) {
        let th = self.text.h_scale / 100.0;
        let tx = (w_text * self.text.font_size + self.text.char_sp) * th;
        let tr = Matrix([1.0,0.0,0.0,1.0,tx,0.0]);
        self.text.tm = self.text.tm.concat(&tr);
    }

    fn advance_default(&mut self, n: usize) {
        for _ in 0..n { self.advance(0.5); }
    }

    fn kern(&mut self, k: f32) {
        let th = self.text.h_scale / 100.0;
        let tx = -k / 1000.0 * self.text.font_size * th;
        let tr = Matrix([1.0,0.0,0.0,1.0,tx,0.0]);
        self.text.tm = self.text.tm.concat(&tr);
    }
}

// ── ttf-parser 轮廓 → tiny-skia 路径（边构造时即变换到设备空间）──────────────────

struct OutlineToPath {
    segs: Vec<Seg>,
    m:    Matrix,
    cur:  Point,
}

impl OutlineToPath {
    fn finish(&self) -> Option<tiny_skia::Path> {
        build_path_identity(&self.segs, &self.m)
    }
}

impl ttf_parser::OutlineBuilder for OutlineToPath {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = Point { x, y }; self.cur = p; self.segs.push(Seg::Move(p));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = Point { x, y }; self.cur = p; self.segs.push(Seg::Line(p));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        // 二次贝塞尔 → 三次：CP1 = cur + 2/3(Q-cur)，CP2 = end + 2/3(Q-end)
        let q = Point { x: x1, y: y1 };
        let e = Point { x, y };
        let c1 = Point { x: self.cur.x + 2.0/3.0*(q.x-self.cur.x), y: self.cur.y + 2.0/3.0*(q.y-self.cur.y) };
        let c2 = Point { x: e.x + 2.0/3.0*(q.x-e.x), y: e.y + 2.0/3.0*(q.y-e.y) };
        self.segs.push(Seg::Cubic(c1, c2, e)); self.cur = e;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let e = Point { x, y };
        self.segs.push(Seg::Cubic(Point{x:x1,y:y1}, Point{x:x2,y:y2}, e)); self.cur = e;
    }
    fn close(&mut self) { self.segs.push(Seg::Close); }
}

// ── 路径构建工具 ──────────────────────────────────────────────────────────────

fn build_path(segs: &[Seg], m: &Matrix) -> Option<tiny_skia::Path> {
    build_path_identity(segs, m)
}

/// 用矩阵 `m` 将各段变换到设备空间后构建 tiny-skia 路径。
fn build_path_identity(segs: &[Seg], m: &Matrix) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for seg in segs {
        match seg {
            Seg::Move(p)          => { let d = m.transform_point(*p); pb.move_to(d.x, d.y); }
            Seg::Line(p)          => { let d = m.transform_point(*p); pb.line_to(d.x, d.y); }
            Seg::Cubic(c1, c2, e) => {
                let (a, b, c) = (m.transform_point(*c1), m.transform_point(*c2), m.transform_point(*e));
                pb.cubic_to(a.x, a.y, b.x, b.y, c.x, c.y);
            }
            Seg::Close            => pb.close(),
        }
    }
    pb.finish()
}

// ── 数值/颜色辅助 ─────────────────────────────────────────────────────────────

fn f(o: &Object) -> f32 {
    match o {
        Object::Integer(n) => *n as f32,
        Object::Real(r)    => *r,
        _ => 0.0,
    }
}

fn pt(a: &Object, b: &Object) -> Point { Point { x: f(a), y: f(b) } }

fn mat6(o: &[Object]) -> Matrix {
    Matrix([f(&o[0]), f(&o[1]), f(&o[2]), f(&o[3]), f(&o[4]), f(&o[5])])
}

fn gray(v: f32) -> Color {
    Color::from_rgba(v.clamp(0.0,1.0), v.clamp(0.0,1.0), v.clamp(0.0,1.0), 1.0).unwrap()
}

fn rgb(o: &[Object]) -> Color {
    Color::from_rgba(f(&o[0]).clamp(0.0,1.0), f(&o[1]).clamp(0.0,1.0), f(&o[2]).clamp(0.0,1.0), 1.0).unwrap()
}

fn cmyk(o: &[Object]) -> Color {
    let (c, m, y, k) = (f(&o[0]), f(&o[1]), f(&o[2]), f(&o[3]));
    let r = (1.0-c)*(1.0-k);
    let g = (1.0-m)*(1.0-k);
    let b = (1.0-y)*(1.0-k);
    Color::from_rgba(r.clamp(0.0,1.0), g.clamp(0.0,1.0), b.clamp(0.0,1.0), 1.0).unwrap()
}

/// sc/scn：按数值操作数个数推断颜色空间（忽略末尾的图案名）。
fn scn(o: &[Object]) -> Option<Color> {
    let nums: Vec<f32> = o.iter().filter(|x| matches!(x, Object::Integer(_) | Object::Real(_))).map(f).collect();
    match nums.len() {
        1 => Some(gray(nums[0])),
        3 => Color::from_rgba(nums[0].clamp(0.0,1.0), nums[1].clamp(0.0,1.0), nums[2].clamp(0.0,1.0), 1.0),
        4 => {
            let (c,m,y,k) = (nums[0],nums[1],nums[2],nums[3]);
            Color::from_rgba(((1.0-c)*(1.0-k)).clamp(0.0,1.0), ((1.0-m)*(1.0-k)).clamp(0.0,1.0), ((1.0-y)*(1.0-k)).clamp(0.0,1.0), 1.0)
        }
        _ => None,
    }
}

/// CTM 的近似均匀缩放比（用于换算线宽到设备空间）。
fn ctm_scale(m: &Matrix) -> f32 {
    let [a, b, c, d, ..] = m.0;
    let sx = (a*a + b*b).sqrt();
    let sy = (c*c + d*d).sqrt();
    ((sx + sy) * 0.5).max(0.01)
}

/// 按编码字节宽度切分字符码。
fn decode_codes(bytes: &[u8], two_byte: bool) -> Vec<u32> {
    if two_byte {
        bytes.chunks(2)
            .map(|c| ((c[0] as u32) << 8) | c.get(1).copied().unwrap_or(0) as u32)
            .collect()
    } else {
        bytes.iter().map(|&b| b as u32).collect()
    }
}
