use std::path::{Path, PathBuf};
use vellum_text::{StreamInterpreter, TextPipeline, extract_text};

fn asset_pdfs() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join("vellum-pdf/tests/assets");
    if !dir.exists() { return vec![]; }
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |e| e == "pdf"))
        .collect();
    paths.sort();
    paths
}

// ── 真实 PDF 测试 ──────────────────────────────────────────────────────────────

/// 真实 PDF 每页应能产生至少一个字形，坐标和字号须合理
#[test]
fn real_pdf_glyphs_are_valid() {
    for path in asset_pdfs() {
        let doc = vellum_pdf::Document::open(&path)
            .unwrap_or_else(|e| panic!("open {:?}: {e}", path));
        for page in 0..doc.page_count() {
            let bytes = doc.page_content_stream(page)
                .unwrap_or_else(|e| panic!("{:?} 第 {page} 页: {e}", path));
            let glyphs = StreamInterpreter::new().run(&bytes);

            if page == 0 {
                assert!(!glyphs.is_empty(), "{:?} 第 0 页应有字形", path.file_name().unwrap());
            }
            for g in &glyphs {
                assert!(g.origin.x.is_finite(),   "x 非有限数");
                assert!(g.origin.y.is_finite(),   "y 非有限数");
                assert!(g.font_size.is_finite(),  "字号非有限数");
                assert!(g.font_size >= 0.0,        "字号不能为负");
            }
        }
    }
}

/// 打印真实 PDF 第 0 页前 30 个字形（含 Phase 3 Unicode 映射结果）
#[test]
fn print_real_pdf_glyphs_page0() {
    use vellum_text::UnicodeMapper;
    for path in asset_pdfs() {
        println!("\n══════ {:?} ══════", path.file_name().unwrap());
        let doc   = vellum_pdf::Document::open(&path).unwrap();
        let bytes = doc.page_content_stream(0).unwrap();
        let mut glyphs = StreamInterpreter::new().run(&bytes);

        // Phase 3：加载 ToUnicode CMap 并映射
        let raw_cmaps = doc.page_to_unicode_cmaps(0).unwrap();
        println!("  CMap 字体数：{}", raw_cmaps.len());
        let mapper = UnicodeMapper::new(raw_cmaps);
        mapper.map_glyphs(&mut glyphs);
        let mapped = mapper.mapped_count(&glyphs);

        println!("  第 0 页字形数：{}，已映射：{}", glyphs.len(), mapped);
        println!("  {:>4}  {:>8}  {:>8}  {:>6}  {:<8}  id    unicode",
                 "#", "x", "y", "fs", "font");
        for (i, g) in glyphs.iter().take(30).enumerate() {
            let ch = g.unicode.map(|c| c.to_string()).unwrap_or_else(|| "—".into());
            println!("  {:>4}  {:>8.2}  {:>8.2}  {:>6.1}  {:<8}  0x{:02X}  {}",
                     i + 1, g.origin.x, g.origin.y, g.font_size,
                     g.font_name, g.glyph_id, ch);
        }
        if glyphs.len() > 30 {
            println!("  ... 共 {} 个字形", glyphs.len());
        }
    }
}

/// Phase 2-4 端到端：TextPipeline 对真实 PDF 应返回非空文本块
#[test]
fn real_pdf_pipeline_produces_blocks() {
    for path in asset_pdfs() {
        let doc    = vellum_pdf::Document::open(&path).unwrap();
        let mut pl = TextPipeline::new();
        let blocks = pl.extract_page(&doc, 0).unwrap();
        assert!(!blocks.is_empty(),
            "{:?} 第 0 页应有文本块", path.file_name().unwrap());

        let total_words: usize = blocks.iter()
            .flat_map(|b| b.lines.iter())
            .map(|l| l.words.len())
            .sum();
        assert!(total_words > 0, "应有词");
    }
}

/// 打印完整提取结果：块 → 行 → 词（肉眼检查）
#[test]
fn print_real_pdf_pipeline_page0() {
    for path in asset_pdfs() {
        println!("\n══════ {:?} ══════", path.file_name().unwrap());
        let doc    = vellum_pdf::Document::open(&path).unwrap();
        let mut pl = TextPipeline::new();
        let blocks = pl.extract_page(&doc, 0).unwrap();
        println!("  共 {} 块", blocks.len());
        for (bi, block) in blocks.iter().enumerate() {
            println!("  ┌─ 块 {} ({} 行)  bounds=({:.0},{:.0})-({:.0},{:.0})",
                bi,
                block.lines.len(),
                block.bounds.min.x, block.bounds.min.y,
                block.bounds.max.x, block.bounds.max.y,
            );
            for line in &block.lines {
                let text: String = line.words.iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("  │  y={:.1}  {text}", line.baseline_y);
            }
            println!("  └─");
        }
    }
}

/// Phase 3 统计：映射率应当合理（>50%）
#[test]
fn real_pdf_unicode_mapping_rate() {
    use vellum_text::UnicodeMapper;
    for path in asset_pdfs() {
        let doc   = vellum_pdf::Document::open(&path).unwrap();
        let bytes = doc.page_content_stream(0).unwrap();
        let mut glyphs = StreamInterpreter::new().run(&bytes);
        let raw_cmaps  = doc.page_to_unicode_cmaps(0).unwrap();
        let mapper     = UnicodeMapper::new(raw_cmaps);
        mapper.map_glyphs(&mut glyphs);
        let total  = glyphs.len();
        let mapped = mapper.mapped_count(&glyphs);
        // 只要有字形就要求映射率 > 50%
        if total > 0 {
            let rate = mapped as f64 / total as f64;
            assert!(rate > 0.5,
                "{:?}: 映射率 {:.1}% 过低（{}/{}）",
                path.file_name().unwrap(), rate * 100.0, mapped, total);
        }
    }
}

/// Phase 2-5 完整流水线：extract_text 输出纯文本
#[test]
fn real_pdf_extract_text_non_empty() {
    for path in asset_pdfs() {
        let doc    = vellum_pdf::Document::open(&path).unwrap();
        let mut pl = TextPipeline::new();
        let blocks = pl.extract_page(&doc, 0).unwrap();
        let text   = extract_text(&blocks);
        assert!(!text.trim().is_empty(),
            "{:?} 第 0 页提取文本不应为空", path.file_name().unwrap());
    }
}

/// 打印 extract_text 最终输出（全链路肉眼检查）
#[test]
fn print_real_pdf_extract_text_page0() {
    for path in asset_pdfs() {
        println!("\n══════ {:?} ══════", path.file_name().unwrap());
        let doc    = vellum_pdf::Document::open(&path).unwrap();
        let mut pl = TextPipeline::new();
        let blocks = pl.extract_page(&doc, 0).unwrap();
        println!("  块数：{}，reading_order 序列：{:?}",
            blocks.len(),
            {
                let mut v: Vec<(usize, usize)> = blocks.iter()
                    .enumerate().map(|(i, b)| (i, b.reading_order)).collect();
                v.sort_by_key(|&(_, o)| o);
                v.iter().map(|&(i, _)| i).collect::<Vec<_>>()
            }
        );
        println!("── 提取文本 ──");
        println!("{}", extract_text(&blocks));
    }
}
