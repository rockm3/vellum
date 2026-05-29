use std::path::PathBuf;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "vellum", about = "PDF inspection and text extraction")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Extract and print text from a PDF file
    Extract {
        file: PathBuf,
        /// Page index (0-based); ignored when --all is set
        #[arg(short, long, default_value_t = 0)]
        page: u32,
        /// Extract all pages; page separators go to stderr
        #[arg(short, long)]
        all: bool,
    },
    /// Inspect internal PDF structure
    Inspect {
        file: PathBuf,
        #[command(subcommand)]
        mode: InspectMode,
    },
    /// Generate a debug SVG overlay for a pipeline stage
    Debug {
        file: PathBuf,
        #[arg(short, long, default_value_t = 0)]
        page: u32,
        /// Stage to visualize: glyphs | words | lines | blocks | order
        #[arg(short, long, default_value = "blocks")]
        stage: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Dump glyph→Unicode mapping decisions as JSON
    Mapping {
        file: PathBuf,
        #[arg(short, long, default_value_t = 0)]
        page: u32,
    },
    /// Compare extracted text against a reference plaintext file
    Diff {
        file: PathBuf,
        expected: PathBuf,
        #[arg(short, long, default_value_t = 0)]
        page: u32,
    },
}

#[derive(Subcommand)]
enum InspectMode {
    /// Dump the cross-reference table and trailer
    Xref {
        /// Also show free entries
        #[arg(short, long)]
        free: bool,
    },
    /// Dump the document object tree (not yet implemented)
    Objects,
    /// Dump the raw content stream tokens for a page (not yet implemented)
    Stream {
        #[arg(short, long, default_value_t = 0)]
        page: u32,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Extract { file, page, all } => {
            let doc   = vellum_pdf::Document::open(&file)?;
            let total = doc.page_count();
            let pages: Vec<u32> = if all { (0..total).collect() } else { vec![page] };

            for (i, p) in pages.iter().enumerate() {
                let mut pipeline = vellum_text::TextPipeline::new();
                let blocks = pipeline.extract_page(&doc, *p)?;
                let text   = vellum_text::extract_text(&blocks);

                if all {
                    // 页眉写 stderr，不污染 stdout 管道输出
                    if i > 0 { eprintln!(); }
                    eprintln!("── 第 {}/{} 页 ─────────────────────────────────────", p + 1, total);
                }

                if text.trim().is_empty() {
                    if !all { eprintln!("（第 {} 页无可提取文本）", p + 1); }
                } else {
                    println!("{text}");
                }
            }
        }

        Command::Inspect { file, mode } => {
            match mode {
                InspectMode::Xref { free } => {
                    // Determine file size before opening (for the sink header).
                    let file_len = std::fs::metadata(&file)?.len() as usize;
                    let mut sink = vellum_debug::ConsoleParseSink::new(file_len);
                    if free {
                        sink = sink.show_free();
                    }
                    let _doc = vellum_pdf::Document::open_with(&file, &mut sink)?;
                }
                InspectMode::Objects => {
                    println!("(object tree not yet implemented)");
                }
                InspectMode::Stream { page } => {
                    let doc = vellum_pdf::Document::open(&file)?;
                    let bytes = doc.page_content_stream(page)?;
                    vellum_debug::StreamPrinter::new().print(page, &bytes);
                }
            }
        }

        Command::Debug { file, page, stage, output } => {
            let doc = vellum_pdf::Document::open(&file)?;
            let sink = vellum_debug::SvgOverlaySink::new(595.0, 842.0);
            let mut pipeline = vellum_text::TextPipeline::with_sink(sink);
            let _ = pipeline.extract_page(&doc, page)?;
            println!("debug stage='{stage}' page={page} — pipeline skeleton in place");
            let _ = output;
        }

        Command::Mapping { file, page } => {
            let doc = vellum_pdf::Document::open(&file)?;
            let sink = vellum_debug::JsonTraceSink::new();
            let mut pipeline = vellum_text::TextPipeline::with_sink(sink);
            let _ = pipeline.extract_page(&doc, page)?;
            println!("(mapping extraction not yet implemented)");
        }

        Command::Diff { file, expected, page } => {
            println!("diff {file:?} vs {expected:?} page {page} — not yet implemented");
        }
    }

    Ok(())
}
