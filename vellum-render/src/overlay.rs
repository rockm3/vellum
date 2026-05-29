use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};
use vellum_pdf::Rect;
use vellum_text::TextBlock;

// ── 配置 ──────────────────────────────────────────────────────────────────────

pub struct OverlayConfig {
    /// 每 pt 对应像素数（72 pt = 1 英寸；1.5 ≈ 108 DPI）
    pub dpi:         f32,
    pub show_blocks: bool,
    pub show_lines:  bool,
    pub show_words:  bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self { dpi: 1.5, show_blocks: true, show_lines: true, show_words: true }
    }
}

// ── 结果 ──────────────────────────────────────────────────────────────────────

pub struct DebugOverlay {
    pub pixmap: Pixmap,
}

impl DebugOverlay {
    pub fn save_png(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        self.pixmap.save_png(path).map_err(|e| format!("{e}").into())
    }

    /// 导出未预乘 RGBA 字节供 GPU 纹理使用（如 egui ColorImage）。
    pub fn to_rgba_straight(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixmap.data().len());
        for p in self.pixmap.pixels() {
            let a = p.alpha();
            if a == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else if a == 255 {
                out.extend_from_slice(&[p.red(), p.green(), p.blue(), 255]);
            } else {
                let f = 255.0 / a as f32;
                out.push((p.red()   as f32 * f).min(255.0) as u8);
                out.push((p.green() as f32 * f).min(255.0) as u8);
                out.push((p.blue()  as f32 * f).min(255.0) as u8);
                out.push(a);
            }
        }
        out
    }

    pub fn width(&self)  -> u32 { self.pixmap.width() }
    pub fn height(&self) -> u32 { self.pixmap.height() }
}

// ── 主函数 ────────────────────────────────────────────────────────────────────

/// 将文本块边界框渲染为调试叠加层 Pixmap（白色背景）。
pub fn render_overlay(
    page_w: f32,
    page_h: f32,
    blocks: &[TextBlock],
    cfg: &OverlayConfig,
) -> Option<DebugOverlay> {
    let pw = (page_w * cfg.dpi) as u32;
    let ph = (page_h * cfg.dpi) as u32;
    if pw == 0 || ph == 0 { return None; }

    let mut pm = Pixmap::new(pw, ph)?;
    pm.fill(Color::WHITE);

    let s = cfg.dpi;
    // PDF y 轴向上，屏幕 y 轴向下：flip_y(y) = page_h - y
    let to_sk = |r: &Rect| -> Option<tiny_skia::Rect> {
        tiny_skia::Rect::from_xywh(
            r.min.x * s,
            (page_h - r.max.y) * s,
            r.width() * s,
            r.height() * s,
        )
    };

    // 词：半透明绿色填充 + 细绿边
    if cfg.show_words {
        for block in blocks {
            for line in &block.lines {
                for word in &line.words {
                    if let Some(r) = to_sk(&word.bounds) {
                        fill_rect(&mut pm, r, [0, 160, 0, 35]);
                        stroke_rect(&mut pm, r, [0, 140, 0, 180], 0.6);
                    }
                }
            }
        }
    }

    // 行：半透明蓝色填充 + 细蓝边
    if cfg.show_lines {
        for block in blocks {
            for line in &block.lines {
                if let Some(r) = to_sk(&line.bounds) {
                    fill_rect(&mut pm, r, [0, 60, 220, 30]);
                    stroke_rect(&mut pm, r, [0, 50, 200, 180], 0.8);
                }
            }
        }
    }

    // 块：粗红色边框（无填充，不遮挡内部细节）
    if cfg.show_blocks {
        for block in blocks {
            if let Some(r) = to_sk(&block.bounds) {
                stroke_rect(&mut pm, r, [210, 30, 30, 230], 2.0);
            }
        }
    }

    Some(DebugOverlay { pixmap: pm })
}

// ── 内部绘图工具 ──────────────────────────────────────────────────────────────

fn fill_rect(pm: &mut Pixmap, r: tiny_skia::Rect, rgba: [u8; 4]) {
    let mut p = Paint::default();
    p.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    pm.fill_rect(r, &p, Transform::identity(), None);
}

fn stroke_rect(pm: &mut Pixmap, r: tiny_skia::Rect, rgba: [u8; 4], width: f32) {
    let mut pb = PathBuilder::new();
    pb.move_to(r.left(),  r.top());
    pb.line_to(r.right(), r.top());
    pb.line_to(r.right(), r.bottom());
    pb.line_to(r.left(),  r.bottom());
    pb.close();
    let Some(path) = pb.finish() else { return };

    let mut p = Paint::default();
    p.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    p.anti_alias = true;
    let stroke = Stroke { width, ..Default::default() };
    pm.stroke_path(&path, &p, &stroke, Transform::identity(), None);
}
