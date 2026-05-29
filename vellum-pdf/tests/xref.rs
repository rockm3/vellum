use std::path::{Path, PathBuf};
use vellum_pdf::{Document, xref::XRefTable};

fn asset_pdfs() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets");
    if !dir.exists() {
        return vec![];
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "pdf"))
        .collect();
    paths.sort();
    paths
}

/// 所有 asset PDF 的 XRef 表必须能无错解析
#[test]
fn xref_parses_without_error() {
    for path in asset_pdfs() {
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let table = XRefTable::parse(&bytes)
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

        assert!(table.trailer.size > 0,
            "{}: trailer /Size = 0", path.display());
        assert!(!table.entries.is_empty(),
            "{}: XRef 表为空", path.display());
    }
}

/// 所有 asset PDF 能正常打开且页数 > 0
#[test]
fn document_open_and_page_count() {
    for path in asset_pdfs() {
        let doc = Document::open(&path)
            .unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
        assert!(doc.page_count() > 0,
            "{}: page_count = 0", path.display());
    }
}

/// trailer /Root 指向的对象编号必须存在于 XRef 表中
#[test]
fn trailer_root_entry_exists_in_xref() {
    for path in asset_pdfs() {
        let bytes = std::fs::read(&path).unwrap();
        let table = XRefTable::parse(&bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let (root_obj, _) = table.trailer.root;
        assert!(
            table.entries.contains_key(&root_obj),
            "{}: /Root obj {} 不在 XRef 表中",
            path.display(), root_obj
        );
    }
}

/// XRef 表中活跃条目数量不超过 /Size
#[test]
fn in_use_entry_count_within_size() {
    for path in asset_pdfs() {
        let bytes = std::fs::read(&path).unwrap();
        let table = XRefTable::parse(&bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            table.entries.len() <= table.trailer.size as usize,
            "{}: entries ({}) > /Size ({})",
            path.display(), table.entries.len(), table.trailer.size
        );
    }
}

/// source_len() 与 fs::metadata 一致（mmap 路径正确）
#[test]
fn source_len_matches_file_size() {
    for path in asset_pdfs() {
        let meta_len = std::fs::metadata(&path).unwrap().len() as usize;
        let doc = Document::open(&path).unwrap();
        assert_eq!(
            doc.source_len(), meta_len,
            "{}: source_len 与文件大小不符", path.display()
        );
    }
}
