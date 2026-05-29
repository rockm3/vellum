use std::path::{Path, PathBuf};
use vellum_debug::StreamPrinter;
use vellum_pdf::Document;

// 共用 vellum-pdf 的 PDF 资产目录
fn asset_pdfs() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join("vellum-pdf/tests/assets");
    if !dir.exists() { return vec![]; }
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "pdf"))
        .collect();
    paths.sort();
    paths
}

/// BT/ET 块标记和文字提取
#[test]
fn renders_bt_et_with_text() {
    let content = b"BT\n/F1 12 Tf\n72 720 Td\n(Hello PDF) Tj\nET\n";
    let out = StreamPrinter::new().render(0, content);

    assert!(out.contains("┌ BT"),        "缺少 BT 开始标记");
    assert!(out.contains("└ ET"),        "缺少 ET 结束标记");
    assert!(out.contains("开始文本对象"), "缺少 BT 说明");
    assert!(out.contains("结束文本对象"), "缺少 ET 说明");
    assert!(out.contains("Hello PDF"),   "缺少 Tj 文字内容");
    assert!(out.contains("设置字体和大小"), "缺少 Tf 说明");
}

/// TJ（含间距调整数组）的文字拼合
#[test]
fn renders_tj_array() {
    let content = b"BT\n[(Hel) -20 (lo)] TJ\nET\n";
    let out = StreamPrinter::new().render(0, content);
    assert!(out.contains("Hel"),  "TJ 未提取 Hel");
    assert!(out.contains("lo"),   "TJ 未提取 lo");
}

/// 矩形路径坐标显示
#[test]
fn renders_rect_coordinates() {
    let content = b"100 200 300 400 re\nf\n";
    let out = StreamPrinter::new().render(0, content);
    assert!(out.contains("x=100"), "缺少矩形 x 坐标");
    assert!(out.contains("y=200"), "缺少矩形 y 坐标");
    assert!(out.contains("w=300"), "缺少矩形宽度");
    assert!(out.contains("h=400"), "缺少矩形高度");
    assert!(out.contains("添加矩形路径"), "缺少 re 说明");
}

/// RGB 颜色操作符
#[test]
fn renders_rgb_color() {
    let content = b"1 0 0 rg\n0 0 1 RG\n";
    let out = StreamPrinter::new().render(0, content);
    assert!(out.contains("设置填充 RGB 颜色"), "缺少 rg 说明");
    assert!(out.contains("设置描边 RGB 颜色"), "缺少 RG 说明");
}

/// 保存/恢复图形状态
#[test]
fn renders_save_restore() {
    let content = b"q\nQ\n";
    let out = StreamPrinter::new().render(0, content);
    assert!(out.contains("保存图形状态"), "缺少 q 说明");
    assert!(out.contains("恢复图形状态"), "缺少 Q 说明");
}

/// 指令行号从 1 开始连续
#[test]
fn line_numbers_are_sequential() {
    let content = b"q\nQ\nBT\nET\n";
    let out = StreamPrinter::new().render(0, content);
    assert!(out.contains("   1  "), "第 1 行缺失");
    assert!(out.contains("   2  "), "第 2 行缺失");
    assert!(out.contains("   3  "), "第 3 行缺失");
    assert!(out.contains("   4  "), "第 4 行缺失");
}

/// 打印一段构造好的内容流样例，供肉眼确认格式
#[test]
fn print_sample_stream() {
    let content = concat!(
        "q\n",
        "0.9 0.9 0.9 rg\n",
        "72 708 468 36 re\n",
        "f\n",
        "Q\n",
        "BT\n",
        "/Helvetica 14 Tf\n",
        "72 720 Td\n",
        "(Hello, Vellum!) Tj\n",
        "0 -20 Td\n",
        "[(Spaced ) 30 (text)] TJ\n",
        "ET\n",
        "1 0 0 RG\n",
        "2 w\n",
        "100 100 m\n",
        "400 100 l\n",
        "400 400 l\n",
        "h\n",
        "S\n",
    );
    println!("{}", StreamPrinter::new().render(0, content.as_bytes()));
}

/// 内容流解析失败时输出错误提示而非 panic
#[test]
fn bad_stream_does_not_panic() {
    let out = StreamPrinter::new().render(0, b"\xFF\xFE garbage not valid content");
    // 只要不 panic 即可；lopdf 可能成功（空操作列表）也可能返回错误信息
    let _ = out;
}

// ─ 用真实 PDF 文件测试（需要 tests/assets/ 中有文件）────────────────────────

/// 打印每个真实 PDF 第一页的内容流（肉眼检查用）
#[test]
fn print_real_pdf_page0() {
    for path in asset_pdfs() {
        println!("\n\n══════════════════════════════════════════════════════════════════════════");
        println!(" 文件：{}", path.file_name().unwrap().to_string_lossy());
        println!("══════════════════════════════════════════════════════════════════════════");
        match Document::open(&path) {
            Ok(doc) => match doc.page_content_stream(0) {
                Ok(bytes) => print!("{}", StreamPrinter::new().render(0, &bytes)),
                Err(e)    => println!("  [获取内容流失败: {e}]"),
            },
            Err(e) => println!("  [打开文件失败: {e}]"),
        }
    }
}

/// 真实 PDF：内容流非空且包含表头
#[test]
fn real_pdf_stream_non_empty() {
    for path in asset_pdfs() {
        let doc = Document::open(&path)
            .unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
        let bytes = doc.page_content_stream(0)
            .unwrap_or_else(|e| panic!("content {}: {e}", path.display()));
        let out = StreamPrinter::new().render(0, &bytes);

        assert!(!out.is_empty(), "{}: 输出为空", path.display());
        assert!(out.contains("第 0 页内容流"), "{}: 缺少标题", path.display());
        assert!(out.contains("指令"), "{}: 缺少表头", path.display());
    }
}

/// 真实 PDF：每一页都能无错渲染
#[test]
fn real_pdf_all_pages_render() {
    for path in asset_pdfs() {
        let doc = Document::open(&path)
            .unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
        for page in 0..doc.page_count() {
            let bytes = doc.page_content_stream(page)
                .unwrap_or_else(|e| panic!("{} 第{page}页: {e}", path.display()));
            let out = StreamPrinter::new().render(page, &bytes);
            assert!(!out.is_empty());
        }
    }
}
