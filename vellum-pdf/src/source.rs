use std::fs::File;
use std::path::Path;
use memmap2::Mmap;
use crate::Result;

/// PDF 文档的字节来源。
///
/// `Mapped` 变体将文件映射到虚拟内存，操作系统按需换入页面，
/// 打开 500 页的 PDF 几乎瞬时完成，只有实际访问的页面才产生 I/O。
pub enum Source {
    /// 内存映射文件。`_file` 必须保持存活以维持映射有效性。
    Mapped { _file: File, mmap: Mmap },
    /// 拥有所有权的字节缓冲区，用于测试或内存中的文档。
    Bytes(Vec<u8>),
}

impl Source {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        // SAFETY: 持有 `_file` 保证映射期间文件描述符有效。
        // 并发写入文件会导致未定义行为，对只读 PDF 可接受此假设。
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(Source::Mapped { _file: file, mmap })
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Source::Bytes(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Source::Mapped { mmap, .. } => mmap,
            Source::Bytes(v) => v,
        }
    }

    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
