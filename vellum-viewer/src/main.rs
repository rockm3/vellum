use eframe::egui::{self, pos2, Color32, FontId, Rect, ScrollArea, Sense, Stroke, Vec2};
use vellum_pdf::Document;
use vellum_text::{TextBlock, TextPipeline};

// ── 入口 ──────────────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let pdf_path = std::env::args().nth(1).unwrap_or_default();
    let options  = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 900.0])
            .with_title("Vellum Debug Viewer"),
        ..Default::default()
    };
    eframe::run_native(
        "Vellum Debug Viewer",
        options,
        Box::new(move |_cc| Ok(Box::new(Viewer::new(pdf_path)))),
    )
}

// ── App 状态 ──────────────────────────────────────────────────────────────────

struct Viewer {
    // 文件
    path_input: String,
    doc:        Option<Document>,
    error:      Option<String>,

    // 分页
    page_idx:   u32,
    page_count: u32,

    // 页面尺寸（pt）
    page_w: f32,
    page_h: f32,

    // 提取结果
    blocks: Vec<TextBlock>,

    // 显示控制
    zoom:        f32,
    show_blocks: bool,
    show_lines:  bool,
    show_words:  bool,
}

impl Viewer {
    fn new(path: String) -> Self {
        let mut v = Self {
            path_input:  path,
            doc:         None,
            error:       None,
            page_idx:    0,
            page_count:  0,
            page_w:      595.0,
            page_h:      842.0,
            blocks:      vec![],
            zoom:        1.0,
            show_blocks: true,
            show_lines:  true,
            show_words:  true,
        };
        if !v.path_input.is_empty() { v.open(); }
        v
    }

    fn open(&mut self) {
        self.error = None;
        match Document::open(std::path::Path::new(&self.path_input)) {
            Ok(doc) => {
                self.page_count = doc.page_count();
                self.page_idx   = 0;
                self.doc        = Some(doc);
                self.extract();
            }
            Err(e) => self.error = Some(format!("打开失败：{e}")),
        }
    }

    fn go_to(&mut self, page: u32) {
        if page < self.page_count {
            self.page_idx = page;
            self.extract();
        }
    }

    fn extract(&mut self) {
        let Some(doc) = &self.doc else { return };
        if let Ok((w, h)) = doc.page_size(self.page_idx) {
            self.page_w = w;
            self.page_h = h;
        }
        match TextPipeline::new().extract_page(doc, self.page_idx) {
            Ok(blocks) => { self.blocks = blocks; self.error = None; }
            Err(e)     => self.error = Some(format!("提取失败：{e}")),
        }
    }
}

// ── UI ────────────────────────────────────────────────────────────────────────

impl eframe::App for Viewer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── 顶部工具栏 ────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("📄");
                let r = ui.add_sized(
                    [360.0, 20.0],
                    egui::TextEdit::singleline(&mut self.path_input).hint_text("PDF 路径"),
                );
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.open();
                }
                if ui.button("打开").clicked() { self.open(); }

                ui.separator();

                // 翻页
                let prev_en = self.page_idx > 0;
                let next_en = self.page_count > 0 && self.page_idx + 1 < self.page_count;
                if ui.add_enabled(prev_en, egui::Button::new("◀")).clicked() {
                    self.go_to(self.page_idx - 1);
                }
                ui.label(format!(
                    "{} / {}",
                    if self.page_count > 0 { self.page_idx + 1 } else { 0 },
                    self.page_count
                ));
                if ui.add_enabled(next_en, egui::Button::new("▶")).clicked() {
                    self.go_to(self.page_idx + 1);
                }

                ui.separator();

                // 缩放
                if ui.small_button("−").clicked() { self.zoom = (self.zoom - 0.1).max(0.2); }
                ui.label(format!("{:.0}%", self.zoom * 100.0));
                if ui.small_button("+").clicked() { self.zoom = (self.zoom + 0.1).min(4.0); }

                ui.separator();

                // 图层开关（带色块图例）
                legend_checkbox(ui, &mut self.show_blocks, "块",  Color32::from_rgb(210, 40, 40));
                legend_checkbox(ui, &mut self.show_lines,  "行",  Color32::from_rgb(40, 80, 210));
                legend_checkbox(ui, &mut self.show_words,  "词",  Color32::from_rgb(30, 160, 30));

                // 错误提示
                if let Some(e) = &self.error {
                    ui.separator();
                    ui.colored_label(Color32::RED, e);
                }
            });
        });

        // ── 右侧文本面板 ──────────────────────────────────────────────────────
        egui::SidePanel::right("text_panel")
            .min_width(280.0)
            .max_width(400.0)
            .show(ctx, |ui| {
                ui.heading("提取文本");
                ui.separator();
                ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    let mut sorted = self.blocks.iter().collect::<Vec<_>>();
                    sorted.sort_by_key(|b| b.reading_order);
                    for block in sorted {
                        let hdr = format!(
                            "块 {}  ({} 行)",
                            block.reading_order,
                            block.lines.len()
                        );
                        ui.colored_label(Color32::from_rgb(180, 30, 30), hdr);
                        for line in &block.lines {
                            let txt = line.words.iter()
                                .map(|w| w.text.as_str())
                                .collect::<Vec<_>>()
                                .join(" ");
                            ui.label(&txt);
                        }
                        ui.add_space(4.0);
                        ui.separator();
                    }
                });
            });

        // ── 中央页面画布 ──────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            // 键盘翻页
            if ui.input(|i| i.key_pressed(egui::Key::ArrowLeft))  { self.go_to(self.page_idx.saturating_sub(1)); }
            if ui.input(|i| i.key_pressed(egui::Key::ArrowRight)) { self.go_to(self.page_idx + 1); }

            // 滚轮缩放
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 { self.zoom = (self.zoom + scroll * 0.001).clamp(0.2, 4.0); }

            if self.page_count == 0 {
                ui.centered_and_justified(|ui| {
                    ui.label("请在上方输入 PDF 路径并点击「打开」");
                });
                return;
            }

            // 计算缩放比例：让页面适应可用空间
            let avail  = ui.available_size();
            let scale  = (avail.x / self.page_w).min(avail.y / self.page_h) * self.zoom;
            let canvas = Vec2::new(self.page_w * scale, self.page_h * scale);

            // 居中偏移
            let pad = ((avail - canvas) * 0.5).max(Vec2::ZERO);
            ui.add_space(pad.y);
            ui.horizontal(|ui| {
                ui.add_space(pad.x);
                let (resp, painter) = ui.allocate_painter(canvas, Sense::hover());
                let origin = resp.rect.min;

                // 白色页面背景 + 阴影边框
                painter.rect_filled(resp.rect, 0.0, Color32::WHITE);
                painter.rect_stroke(resp.rect, 0.0, Stroke::new(1.5, Color32::from_gray(160)));

                // PDF pt → 屏幕像素（flip Y：PDF y 向上，屏幕 y 向下）
                let to_screen = |r: &vellum_pdf::Rect| -> Rect {
                    Rect::from_min_max(
                        pos2(origin.x + r.min.x * scale,
                             origin.y + (self.page_h - r.max.y) * scale),
                        pos2(origin.x + r.max.x * scale,
                             origin.y + (self.page_h - r.min.y) * scale),
                    )
                };

                // 词（最底层）
                if self.show_words {
                    for block in &self.blocks {
                        for line in &block.lines {
                            for word in &line.words {
                                let r = to_screen(&word.bounds);
                                painter.rect_filled(r, 1.0, Color32::from_rgba_unmultiplied(30, 160, 30, 40));
                                painter.rect_stroke(r, 1.0, Stroke::new(0.6, Color32::from_rgba_unmultiplied(20, 140, 20, 180)));
                            }
                        }
                    }
                }

                // 行
                if self.show_lines {
                    for block in &self.blocks {
                        for line in &block.lines {
                            let r = to_screen(&line.bounds);
                            painter.rect_filled(r, 1.0, Color32::from_rgba_unmultiplied(40, 80, 220, 30));
                            painter.rect_stroke(r, 1.0, Stroke::new(0.8, Color32::from_rgba_unmultiplied(30, 60, 200, 180)));
                        }
                    }
                }

                // 块（最顶层：粗红框 + 阅读序号）
                if self.show_blocks {
                    let fsize = (10.0 * scale).clamp(8.0, 14.0);
                    for block in &self.blocks {
                        let r = to_screen(&block.bounds);
                        painter.rect_stroke(r, 2.0, Stroke::new(2.0, Color32::from_rgba_unmultiplied(210, 30, 30, 220)));
                        painter.text(
                            r.min + Vec2::new(2.0, 1.0),
                            egui::Align2::LEFT_TOP,
                            block.reading_order.to_string(),
                            FontId::monospace(fsize),
                            Color32::from_rgb(180, 0, 0),
                        );
                    }
                }
            });
        });
    }
}

// ── 小工具 ────────────────────────────────────────────────────────────────────

/// 带色块图例的 checkbox
fn legend_checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str, color: Color32) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
        ui.painter().rect_filled(rect, 2.0, color);
        ui.checkbox(checked, label);
    });
}
