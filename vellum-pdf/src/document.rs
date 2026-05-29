use std::path::Path;
use crate::inspect::{NullParseSink, ParseSink};
use crate::source::Source;
use crate::xref::XRefTable;
use crate::{Error, Result};

pub struct Document {
    source: Source,
    pub(crate) xref: XRefTable,
    /// 对象级访问暂由 lopdf 负责，待实现自有懒加载解析器后替换。
    inner: lopdf::Document,
}

impl Document {
    /// 打开并解析一个 PDF 文件。仅立即解析 XRef 表，对象数据由 lopdf 按需加载。
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with(path, &mut NullParseSink)
    }

    /// 同 [`open`]，但在 XRef 解析过程中将 [`ParseEvent`] 发送至 `sink`。
    pub fn open_with(path: &Path, sink: &mut dyn ParseSink) -> Result<Self> {
        let source = Source::open(path)?;
        let xref   = XRefTable::parse_with(source.as_bytes(), sink)?;
        // 复用已映射的字节，文件只读取一次
        let inner  = lopdf::Document::load_mem(source.as_bytes())?;
        Ok(Self { source, xref, inner })
    }

    pub fn page_count(&self) -> u32 {
        self.inner.get_pages().len() as u32
    }

    pub fn page_id(&self, index: u32) -> Result<lopdf::ObjectId> {
        let page_num = index + 1; // lopdf 使用从 1 开始的页面编号
        self.inner
            .get_pages()
            .get(&page_num)
            .copied()
            .ok_or(Error::PageNotFound { index })
    }

    /// 返回指定页面内容流的原始字节（lopdf 会自动解压缩）。
    /// 多个内容流 array 会被合并为一段连续字节。
    pub fn page_content_stream(&self, page_index: u32) -> Result<Vec<u8>> {
        let page_id = self.page_id(page_index)?;
        Ok(self.inner.get_page_content(page_id)?)
    }

    /// 返回页面所有字体的 ToUnicode CMap 内容字节（已解压）。
    ///
    /// key 格式与页面资源字典一致，如 `"/F4"`。
    /// 不含 ToUnicode 条目的字体直接跳过，不报错。
    pub fn page_to_unicode_cmaps(
        &self,
        page_index: u32,
    ) -> Result<std::collections::HashMap<String, Vec<u8>>> {
        let page_id = self.page_id(page_index)?;
        let mut result = std::collections::HashMap::new();

        let font_dict = match self.page_font_dict(page_id) {
            Some(d) => d,
            None    => return Ok(result),
        };

        for (name_bytes, font_val) in font_dict.iter() {
            let key = format!("/{}", String::from_utf8_lossy(name_bytes));

            let font_obj = match resolve(&self.inner, font_val) {
                Some(o) => o,
                None    => continue,
            };
            let fd = match font_obj.as_dict() {
                Ok(d)  => d,
                Err(_) => continue,
            };
            let to_u_val = match fd.get(b"ToUnicode") {
                Ok(v)  => v,
                Err(_) => continue,
            };
            let stream_obj = match resolve(&self.inner, to_u_val) {
                Some(o) => o,
                None    => continue,
            };
            if let lopdf::Object::Stream(s) = stream_obj {
                if let Ok(bytes) = s.decompressed_content() {
                    result.insert(key, bytes);
                }
            }
        }

        Ok(result)
    }

    /// 返回页面字体资源字典（借用 lopdf 内部对象）。
    fn page_font_dict(&self, page_id: lopdf::ObjectId) -> Option<&lopdf::Dictionary> {
        let page_obj  = self.inner.get_object(page_id).ok()?;
        let page_dict = page_obj.as_dict().ok()?;

        let res_val  = page_dict.get(b"Resources").ok()?;
        let res_dict = resolve(&self.inner, res_val)?.as_dict().ok()?;

        let font_val = res_dict.get(b"Font").ok()?;
        resolve(&self.inner, font_val)?.as_dict().ok()
    }

    pub fn xref(&self) -> &XRefTable { &self.xref }
    pub fn source_len(&self) -> usize { self.source.len() }
}

/// Reference 透明解引用：若对象是 Reference 则跟随一级，否则原样返回。
fn resolve<'a>(doc: &'a lopdf::Document, obj: &'a lopdf::Object) -> Option<&'a lopdf::Object> {
    match obj {
        lopdf::Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}
