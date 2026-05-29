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

    /// 提取页面所有字体的渲染数据（Type3 CharProc / 嵌入 TrueType 程序）。
    ///
    /// key 与页面资源字典一致，如 `"/F4"`。无法识别的字体记为 [`Font::Unsupported`]。
    pub fn page_fonts(&self, page_index: u32) -> Result<crate::PageFonts> {
        use crate::Font;
        let page_id   = self.page_id(page_index)?;
        let mut fonts = std::collections::HashMap::new();

        let Some(font_dict) = self.page_font_dict(page_id) else { return Ok(fonts) };

        for (name_bytes, font_val) in font_dict.iter() {
            let key = format!("/{}", String::from_utf8_lossy(name_bytes));
            let font = self.extract_font(font_val).unwrap_or(Font::Unsupported);
            fonts.insert(key, font);
        }
        Ok(fonts)
    }

    fn extract_font(&self, font_val: &lopdf::Object) -> Option<crate::Font> {
        use crate::Font;
        let fd = resolve(&self.inner, font_val)?.as_dict().ok()?;
        let subtype = fd.get(b"Subtype").ok()?.as_name().ok()?;

        match subtype {
            b"Type3" => self.extract_type3(fd),
            b"Type0" => self.extract_type0(fd),
            b"TrueType" => {
                let prog = self.font_descriptor_program(fd)?;
                Some(Font::TrueType { program: prog, two_byte: false })
            }
            _ => None,
        }
    }

    /// Type3：FontMatrix + Encoding/Differences + CharProcs。
    fn extract_type3(&self, fd: &lopdf::Dictionary) -> Option<crate::Font> {
        use crate::Font;

        let fm_arr = fd.get(b"FontMatrix").ok()?.as_array().ok()?;
        let mut font_matrix = [0.0f32; 6];
        for (i, o) in fm_arr.iter().take(6).enumerate() {
            font_matrix[i] = num(o);
        }

        // Encoding/Differences：单字节编码 → 字形名
        let enc = resolve(&self.inner, fd.get(b"Encoding").ok()?)?.as_dict().ok()?;
        let diffs = enc.get(b"Differences").ok()?.as_array().ok()?;
        let mut code_to_name: std::collections::HashMap<u8, Vec<u8>> = std::collections::HashMap::new();
        let mut cur: i64 = 0;
        for o in diffs {
            match o {
                lopdf::Object::Integer(n) => cur = *n,
                lopdf::Object::Name(name) => {
                    if (0..=255).contains(&cur) {
                        code_to_name.insert(cur as u8, name.clone());
                    }
                    cur += 1;
                }
                _ => {}
            }
        }

        // CharProcs：字形名 → 内容流
        let char_procs_dict = resolve(&self.inner, fd.get(b"CharProcs").ok()?)?.as_dict().ok()?;
        let mut char_procs = std::collections::HashMap::new();
        for (code, name) in &code_to_name {
            if let Ok(proc_val) = char_procs_dict.get(name) {
                if let Some(lopdf::Object::Stream(s)) = resolve(&self.inner, proc_val) {
                    if let Ok(bytes) = s.decompressed_content() {
                        char_procs.insert(*code, bytes);
                    }
                }
            }
        }

        // Widths（按 FirstChar 起算）
        let mut widths = std::collections::HashMap::new();
        if let (Ok(first), Ok(w_arr)) = (fd.get(b"FirstChar"), fd.get(b"Widths")) {
            if let (lopdf::Object::Integer(fc), Ok(arr)) = (first, w_arr.as_array()) {
                for (i, o) in arr.iter().enumerate() {
                    let code = *fc + i as i64;
                    if (0..=255).contains(&code) {
                        widths.insert(code as u8, num(o));
                    }
                }
            }
        }

        Some(Font::Type3 { font_matrix, char_procs, widths })
    }

    /// Type0：取 DescendantFont 的 FontFile2，判断是否 Identity 双字节。
    fn extract_type0(&self, fd: &lopdf::Dictionary) -> Option<crate::Font> {
        use crate::Font;

        let two_byte = fd.get(b"Encoding").ok()
            .and_then(|o| o.as_name().ok())
            .map(|n| n.starts_with(b"Identity"))
            .unwrap_or(false);

        let df = resolve(&self.inner, fd.get(b"DescendantFonts").ok()?)?;
        let cidfont = match df {
            lopdf::Object::Array(a) => resolve(&self.inner, a.first()?)?.as_dict().ok()?,
            lopdf::Object::Dictionary(d) => d,
            _ => return None,
        };
        let program = self.font_descriptor_program(cidfont)?;
        Some(Font::TrueType { program, two_byte })
    }

    /// 从字体字典的 FontDescriptor 取 FontFile2 字节。
    fn font_descriptor_program(&self, fd: &lopdf::Dictionary) -> Option<Vec<u8>> {
        let descriptor = resolve(&self.inner, fd.get(b"FontDescriptor").ok()?)?.as_dict().ok()?;
        let ff = descriptor.get(b"FontFile2").ok()?;
        if let Some(lopdf::Object::Stream(s)) = resolve(&self.inner, ff) {
            return s.decompressed_content().ok();
        }
        None
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

    /// 返回页面的物理尺寸（宽、高，单位 pt）。
    ///
    /// 读取页面字典的 `MediaBox` 条目；若不存在则返回标准 A4（595 × 842 pt）。
    pub fn page_size(&self, page_index: u32) -> Result<(f32, f32)> {
        let page_id   = self.page_id(page_index)?;
        let page_obj  = self.inner.get_object(page_id)?;
        let page_dict = page_obj.as_dict()?;

        if let Ok(lopdf::Object::Array(arr)) = page_dict.get(b"MediaBox") {
            if arr.len() >= 4 {
                let n = |o: &lopdf::Object| match o {
                    lopdf::Object::Integer(i) => *i as f32,
                    lopdf::Object::Real(f)    => *f as f32,
                    _ => 0.0,
                };
                let (x0, y0, x1, y1) = (n(&arr[0]), n(&arr[1]), n(&arr[2]), n(&arr[3]));
                let (w, h) = (x1 - x0, y1 - y0);
                if w > 0.0 && h > 0.0 { return Ok((w, h)); }
            }
        }
        Ok((595.0, 842.0)) // 默认 A4
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

/// Object → f32（Integer / Real，其余为 0）。
fn num(o: &lopdf::Object) -> f32 {
    match o {
        lopdf::Object::Integer(n) => *n as f32,
        lopdf::Object::Real(f)    => *f as f32,
        _ => 0.0,
    }
}
