use std::collections::BTreeMap;
use crate::inspect::{NullParseSink, ParseEvent, ParseSink};
use crate::{Error, Result};

/// `(对象编号, 世代编号)` / `(object_number, generation_number)`
pub type ObjectId = (u32, u16);

#[derive(Debug, Clone)]
pub enum XRefEntry {
    /// 已释放的对象槽，`next_free` 指向下一个空闲对象编号
    Free { next_free: u32, generation: u16 },
    /// 活跃对象，位于文件中 `offset` 字节处
    InUse { offset: usize, generation: u16 },
    /// 压缩对象，嵌入对象流内（PDF 1.5+）
    Compressed { container_id: u32, index: u32 },
}

#[derive(Debug, Clone)]
pub struct Trailer {
    /// `/Root` — 文档目录对象
    pub root: ObjectId,
    /// `/Info` — 可选的文档信息字典
    pub info: Option<ObjectId>,
    /// `/Size` — 对象总数（最大对象编号 + 1）
    pub size: u32,
    /// `/Prev` — 上一个 XRef 节区的字节偏移（增量更新时存在）
    pub prev: Option<usize>,
}

#[derive(Debug)]
pub struct XRefTable {
    pub entries: BTreeMap<u32, XRefEntry>,
    pub trailer: Trailer,
    /// 本 XRef 节区在文件中的起始字节偏移
    pub xref_offset: usize,
}

impl XRefTable {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::parse_with(bytes, &mut NullParseSink)
    }

    pub fn parse_with(bytes: &[u8], sink: &mut dyn ParseSink) -> Result<Self> {
        let xref_offset = locate_startxref(bytes)?;
        sink.on_parse_event(ParseEvent::StartXRef { offset: xref_offset });

        let xref_slice = bytes.get(xref_offset..).ok_or(Error::MalformedContent)?;

        let (entries, trailer) = if xref_slice.starts_with(b"xref") {
            parse_traditional(xref_slice, sink)?
        } else {
            // PDF 1.5+ 的交叉引用流，暂不自行解析，由调用方使用 lopdf 处理
            return Err(Error::UnsupportedFeature("XRef streams (PDF 1.5+)"));
        };

        sink.on_parse_event(ParseEvent::Trailer(&trailer));
        Ok(Self { entries, trailer, xref_offset })
    }
}

fn parse_traditional(
    xref_slice: &[u8],
    sink: &mut dyn ParseSink,
) -> Result<(BTreeMap<u32, XRefEntry>, Trailer)> {
    let mut pos = 4; // 跳过 "xref" 关键字
    let mut entries: BTreeMap<u32, XRefEntry> = BTreeMap::new();

    loop {
        pos += ws_len(&xref_slice[pos..]);

        if xref_slice[pos..].starts_with(b"trailer") {
            pos += 7;
            break;
        }

        // 每个子节区头格式：`<起始对象编号> <条目数>\n`
        let (first, rest) = parse_uint(&xref_slice[pos..]).ok_or(Error::MalformedContent)?;
        let rest = skip_ws(rest);
        let (count, rest) = parse_uint(rest).ok_or(Error::MalformedContent)?;
        pos = xref_slice.len() - rest.len();
        pos += ws_len(&xref_slice[pos..]);

        for i in 0..count {
            let obj_num = (first + i) as u32;
            // 规范要求每条记录恰好 20 字节：10位偏移 + 空格 + 5位世代 + 空格 + 1位类型 + 行结束符(2)
            let entry_bytes = xref_slice.get(pos..pos + 20).ok_or(Error::MalformedContent)?;
            let entry = parse_entry(entry_bytes)?;
            let g = entry_generation(&entry);
            entries.insert(obj_num, entry);
            sink.on_parse_event(ParseEvent::XRefEntry {
                id: (obj_num, g),
                entry: entries.get(&obj_num).unwrap(),
            });
            pos += 20;
        }
    }

    pos += ws_len(&xref_slice[pos..]);
    let trailer = parse_trailer(&xref_slice[pos..])?;
    Ok((entries, trailer))
}

fn entry_generation(e: &XRefEntry) -> u16 {
    match e {
        XRefEntry::InUse { generation, .. } | XRefEntry::Free { generation, .. } => *generation,
        XRefEntry::Compressed { .. } => 0,
    }
}

/// 解析一条 20 字节的 XRef 记录
fn parse_entry(e: &[u8]) -> Result<XRefEntry> {
    if e.len() < 18 {
        return Err(Error::MalformedContent);
    }
    let offset: usize = ascii_digits_to_uint(&e[0..10])?;
    let g: u16        = ascii_digits_to_uint(&e[11..16])? as u16;
    match e[17] {
        b'n' => Ok(XRefEntry::InUse { offset,               generation: g }),
        b'f' => Ok(XRefEntry::Free  { next_free: offset as u32, generation: g }),
        _    => Err(Error::MalformedContent),
    }
}

fn ascii_digits_to_uint(s: &[u8]) -> Result<usize> {
    std::str::from_utf8(s)
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .ok_or(Error::MalformedContent)
}

fn parse_trailer(bytes: &[u8]) -> Result<Trailer> {
    let start = find(bytes, b"<<").ok_or(Error::MalformedContent)?;
    let body  = &bytes[start + 2..];

    let size = extract_uint(body, b"/Size").ok_or(Error::MalformedContent)? as u32;
    let root = extract_ref(body, b"/Root").ok_or(Error::MalformedContent)?;
    let info = extract_ref(body, b"/Info");
    let prev = extract_uint(body, b"/Prev");

    Ok(Trailer { root, info, size, prev })
}

fn extract_uint(dict: &[u8], key: &[u8]) -> Option<usize> {
    let pos = find(dict, key)?;
    let (v, _) = parse_uint(skip_ws(&dict[pos + key.len()..]))?;
    Some(v)
}

fn extract_ref(dict: &[u8], key: &[u8]) -> Option<ObjectId> {
    let pos = find(dict, key)?;
    let rest = skip_ws(&dict[pos + key.len()..]);
    let (obj, rest) = parse_uint(rest)?;
    let rest = skip_ws(rest);
    let (g, rest) = parse_uint(rest)?;
    let rest = skip_ws(rest);
    if rest.first() != Some(&b'R') { return None; }
    Some((obj as u32, g as u16))
}

/// 在文件尾部 1024 字节内倒序查找 `startxref`，返回其后跟随的字节偏移值
fn locate_startxref(bytes: &[u8]) -> Result<usize> {
    let tail_start = bytes.len().saturating_sub(1024);
    let tail = &bytes[tail_start..];
    let local = rfind(tail, b"startxref").ok_or(Error::MalformedContent)?;
    let after = skip_ws(&tail[local + b"startxref".len()..]);
    let (offset, _) = parse_uint(after).ok_or(Error::MalformedContent)?;
    Ok(offset)
}

fn ws_len(s: &[u8]) -> usize {
    s.iter().take_while(|&&b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')).count()
}

fn skip_ws(s: &[u8]) -> &[u8] {
    &s[ws_len(s)..]
}

fn parse_uint(s: &[u8]) -> Option<(usize, &[u8])> {
    let n = s.iter().take_while(|&&b| b.is_ascii_digit()).count();
    if n == 0 { return None; }
    let v: usize = std::str::from_utf8(&s[..n]).ok()?.parse().ok()?;
    Some((v, &s[n..]))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() { return None; }
    (0..=hay.len() - needle.len()).rev().find(|&i| &hay[i..i + needle.len()] == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{NullParseSink, ParseEvent, ParseSink};

    // 最小合法传统 XRef + trailer，xref 节区起始于字节 0
    static MINIMAL: &[u8] = b"\
xref\n\
0 2\n\
0000000000 65535 f\r\n\
0000000015 00000 n\r\n\
trailer\n\
<< /Size 2 /Root 1 0 R >>\n\
startxref\n\
0\n\
%%EOF";

    // 含两个子节区的 XRef 表
    static MULTI_SECTION: &[u8] = b"\
xref\n\
0 1\n\
0000000000 65535 f\r\n\
2 1\n\
0000000042 00000 n\r\n\
trailer\n\
<< /Size 3 /Root 2 0 R >>\n\
startxref\n\
0\n\
%%EOF";

    #[test]
    fn parse_minimal_xref() {
        let table = XRefTable::parse(MINIMAL).unwrap();
        assert_eq!(table.xref_offset, 0);
        assert_eq!(table.trailer.size, 2);
        assert_eq!(table.trailer.root, (1, 0));
        assert_eq!(table.entries.len(), 2);
    }

    #[test]
    fn entry_types_correct() {
        let table = XRefTable::parse(MINIMAL).unwrap();
        assert!(matches!(table.entries[&0], XRefEntry::Free { next_free: 0, generation: 65535 }));
        assert!(matches!(table.entries[&1], XRefEntry::InUse { offset: 15, generation: 0 }));
    }

    #[test]
    fn parse_entry_in_use() {
        let entry = parse_entry(b"0000000042 00003 n\r\n").unwrap();
        assert!(matches!(entry, XRefEntry::InUse { offset: 42, generation: 3 }));
    }

    #[test]
    fn parse_entry_free() {
        let entry = parse_entry(b"0000000007 00000 f\r\n").unwrap();
        assert!(matches!(entry, XRefEntry::Free { next_free: 7, generation: 0 }));
    }

    #[test]
    fn parse_entry_unknown_type_errors() {
        assert!(parse_entry(b"0000000007 00000 x\r\n").is_err());
    }

    #[test]
    fn multi_subsection_xref() {
        let table = XRefTable::parse(MULTI_SECTION).unwrap();
        assert_eq!(table.entries.len(), 2);
        assert_eq!(table.trailer.root, (2, 0));
        assert!(matches!(table.entries[&0], XRefEntry::Free { .. }));
        assert!(matches!(table.entries[&2], XRefEntry::InUse { offset: 42, .. }));
    }

    #[test]
    fn missing_startxref_errors() {
        assert!(XRefTable::parse(b"not a pdf").is_err());
    }

    #[test]
    fn trailer_info_and_prev_optional() {
        static WITH_INFO: &[u8] = b"\
xref\n\
0 1\n\
0000000000 65535 f\r\n\
trailer\n\
<< /Size 1 /Root 1 0 R /Info 2 0 R /Prev 1234 >>\n\
startxref\n\
0\n\
%%EOF";
        let table = XRefTable::parse(WITH_INFO).unwrap();
        assert_eq!(table.trailer.info, Some((2, 0)));
        assert_eq!(table.trailer.prev, Some(1234));
    }

    #[test]
    fn sink_fires_correct_event_sequence() {
        #[derive(Default)]
        struct Counter { start_xref: u32, entries: u32, trailer: u32 }
        impl ParseSink for Counter {
            fn on_parse_event(&mut self, event: ParseEvent<'_>) {
                match event {
                    ParseEvent::StartXRef { .. } => self.start_xref += 1,
                    ParseEvent::XRefEntry { .. } => self.entries += 1,
                    ParseEvent::Trailer(_)        => self.trailer += 1,
                    ParseEvent::ObjectAccess { .. } => {}
                }
            }
        }

        let mut c = Counter::default();
        XRefTable::parse_with(MINIMAL, &mut c).unwrap();
        assert_eq!(c.start_xref, 1);
        assert_eq!(c.entries, 2);
        assert_eq!(c.trailer, 1);

        // NullParseSink 不应影响解析结果
        let t1 = XRefTable::parse(MINIMAL).unwrap();
        let t2 = XRefTable::parse_with(MINIMAL, &mut NullParseSink).unwrap();
        assert_eq!(t1.trailer.root, t2.trailer.root);
        assert_eq!(t1.entries.len(), t2.entries.len());
    }
}
