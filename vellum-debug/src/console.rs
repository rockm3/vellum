use vellum_pdf::inspect::{ParseEvent, ParseSink};
use vellum_pdf::xref::XRefEntry;

/// 在解析过程中将 XRef 表和 trailer 以人类可读格式输出到 stdout。
///
/// 输出示例：
/// ```text
/// startxref → offset 241234  (file: 245678 bytes)
///
/// XRef table
///   obj  gen  type      detail
///   ────  ───  ────────  ────────────────────
///   0001   0  in-use    offset=15
///   0002   0  in-use    offset=208
///   0003   0  free      next=0
///
/// Trailer
///   /Root   1 0 R
///   /Info   2 0 R
///   /Size   245
///   244 in-use   1 free
/// ```
pub struct ConsoleParseSink {
    file_len:  usize,
    in_use:    u32,
    free:      u32,
    show_free: bool,
}

impl ConsoleParseSink {
    pub fn new(file_len: usize) -> Self {
        Self { file_len, in_use: 0, free: 0, show_free: false }
    }

    /// 同时输出空闲条目（默认隐藏）。
    pub fn show_free(mut self) -> Self {
        self.show_free = true;
        self
    }
}

impl ParseSink for ConsoleParseSink {
    fn on_parse_event(&mut self, event: ParseEvent<'_>) {
        match event {
            ParseEvent::StartXRef { offset } => {
                println!("startxref → offset {offset}  (file: {} bytes)", self.file_len);
                println!();
                println!("XRef table");
                println!("  {:>4}  {:>3}  {:<8}  detail", "obj", "gen", "type");
                println!("  {}  {}  {}  {}", "─".repeat(4), "─".repeat(3), "─".repeat(8), "─".repeat(20));
            }

            ParseEvent::XRefEntry { id: (obj, g), entry } => {
                match entry {
                    XRefEntry::InUse { offset, .. } => {
                        println!("  {obj:>4}  {g:>3}  in-use    offset={offset}");
                        self.in_use += 1;
                    }
                    XRefEntry::Free { next_free, .. } => {
                        self.free += 1;
                        if self.show_free || obj != 0 {
                            println!("  {obj:>4}  {g:>3}  free      next={next_free}");
                        }
                    }
                    XRefEntry::Compressed { container_id, index } => {
                        println!("  {obj:>4}  {g:>3}  packed    obj_stream={container_id}[{index}]");
                        self.in_use += 1;
                    }
                }
            }

            ParseEvent::Trailer(t) => {
                println!();
                println!("Trailer");
                println!("  /Root  {} {} R", t.root.0, t.root.1);
                if let Some(info) = t.info {
                    println!("  /Info  {} {} R", info.0, info.1);
                }
                println!("  /Size  {}", t.size);
                if let Some(prev) = t.prev {
                    println!("  /Prev  {prev}  (incremental update)");
                }
                println!();
                println!("  {} in-use   {} free", self.in_use, self.free);
            }

            ParseEvent::ObjectAccess { id: (obj, g), at_offset } => {
                println!("  → load {obj} {g} R  @ offset {at_offset}");
            }
        }
    }
}
