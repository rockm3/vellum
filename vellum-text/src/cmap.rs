use std::collections::HashMap;

/// PDF ToUnicode CMap：glyph ID → Unicode 字符的映射表。
///
/// 只解析 `beginbfchar` / `beginbfrange` 两种映射节；
/// `codespacerange` 等元信息节直接跳过。
pub struct CMap {
    map: HashMap<u32, char>,
}

impl CMap {
    /// 从 ToUnicode CMap 流字节解析映射表。解析失败时返回空表，不 panic。
    pub fn parse(bytes: &[u8]) -> Self {
        let text = String::from_utf8_lossy(bytes);
        let mut map = HashMap::new();
        let mut mode = Section::Other;

        for raw_line in text.lines() {
            let line = raw_line.trim();
            // 用最后一个 token 识别节边界（应对 "10 beginbfchar" 这类写法）
            let last = line.split_ascii_whitespace().next_back().unwrap_or("");
            match last {
                "beginbfchar"  => { mode = Section::BfChar;  continue; }
                "endbfchar"    => { mode = Section::Other;   continue; }
                "beginbfrange" => { mode = Section::BfRange; continue; }
                "endbfrange"   => { mode = Section::Other;   continue; }
                _ => {}
            }
            match mode {
                Section::BfChar  => parse_bfchar(line, &mut map),
                Section::BfRange => parse_bfrange(line, &mut map),
                Section::Other   => {}
            }
        }

        Self { map }
    }

    pub fn get(&self, glyph_id: u32) -> Option<char> {
        self.map.get(&glyph_id).copied()
    }

    pub fn len(&self) -> usize  { self.map.len() }
    pub fn is_empty(&self) -> bool { self.map.is_empty() }
}

#[derive(PartialEq)]
enum Section { Other, BfChar, BfRange }

// ── 行解析 ────────────────────────────────────────────────────────────────────

/// bfchar 行格式：`<srcHex> <dstHex>`
fn parse_bfchar(line: &str, map: &mut HashMap<u32, char>) {
    let tokens = hex_tokens(line);
    if tokens.len() < 2 { return; }
    let src = bytes_to_u32(&tokens[0]);
    if let Some(ch) = bytes_to_char(&tokens[1]) {
        map.insert(src, ch);
    }
}

/// bfrange 行格式：
/// - `<lo> <hi> <startDst>`  — 顺序映射
/// - `<lo> <hi> [<d0> <d1> …]` — 数组映射
fn parse_bfrange(line: &str, map: &mut HashMap<u32, char>) {
    let tokens = hex_tokens(line);
    if tokens.len() < 3 { return; }
    let lo = bytes_to_u32(&tokens[0]);
    let hi = bytes_to_u32(&tokens[1]);
    if lo > hi { return; }

    if line.contains('[') {
        // 数组形式：tokens[2..] 与 [lo..hi] 一一对应
        for (i, tok) in tokens[2..].iter().enumerate() {
            let gid = lo + i as u32;
            if gid > hi { break; }
            if let Some(ch) = bytes_to_char(tok) {
                map.insert(gid, ch);
            }
        }
    } else {
        // 顺序形式：lo→dst, lo+1→dst+1, …
        let start = bytes_to_u32(&tokens[2]);
        for i in 0..=(hi - lo) {
            if let Some(ch) = char::from_u32(start + i) {
                map.insert(lo + i, ch);
            }
        }
    }
}

// ── 十六进制 token 工具 ───────────────────────────────────────────────────────

/// 从文本行中提取所有 `<HH…>` 格式的字节序列。
fn hex_tokens(line: &str) -> Vec<Vec<u8>> {
    let mut tokens = Vec::new();
    let mut rest   = line;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('>') else { break };
        let hex = rest[..close].trim();
        rest = &rest[close + 1..];
        // 跳过嵌套 dict "<<" 产生的空 token 或奇数长度的垃圾
        if !hex.is_empty() {
            if let Some(bytes) = parse_hex(hex) {
                tokens.push(bytes);
            }
        }
    }
    tokens
}

/// 十六进制字符串 → 字节向量（奇数长度前补零）。
fn parse_hex(s: &str) -> Option<Vec<u8>> {
    if s.is_empty() { return None; }
    // 奇数位数：前补一个 '0' 使之偶数
    let owned;
    let s = if s.len() % 2 != 0 {
        owned = format!("0{s}");
        &owned as &str
    } else {
        s
    };
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks(2) {
        let hi = hex_digit(chunk[0])?;
        let lo = hex_digit(chunk[1])?;
        bytes.push((hi << 4) | lo);
    }
    Some(bytes)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 字节序列 → u32（大端，最多 4 字节）。
fn bytes_to_u32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |acc, &b| (acc << 8) | b as u32)
}

/// 字节序列 → Unicode 字符（支持 BMP 2 字节、代理对 4 字节、单字节）。
fn bytes_to_char(bytes: &[u8]) -> Option<char> {
    match bytes.len() {
        1 => char::from_u32(bytes[0] as u32),
        2 => char::from_u32(u16::from_be_bytes([bytes[0], bytes[1]]) as u32),
        4 => {
            let hi = u16::from_be_bytes([bytes[0], bytes[1]]);
            let lo = u16::from_be_bytes([bytes[2], bytes[3]]);
            if (0xD800..0xDC00).contains(&hi) && (0xDC00..0xE000).contains(&lo) {
                // UTF-16BE 代理对 → 补充平面码位
                let cp = 0x10000u32 + ((hi as u32 - 0xD800) << 10) + (lo as u32 - 0xDC00);
                char::from_u32(cp)
            } else {
                char::from_u32(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            }
        }
        _ => None,
    }
}

// ── 单元测试 ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cmap(src: &str) -> CMap {
        CMap::parse(src.as_bytes())
    }

    #[test]
    fn empty_input_gives_empty_map() {
        assert!(cmap("").is_empty());
    }

    #[test]
    fn bfchar_single_byte_src_two_byte_dst() {
        let c = cmap("beginbfchar\n<DE> <6A21>\nendbfchar\n");
        assert_eq!(c.get(0xDE), Some('模'));   // U+6A21
    }

    #[test]
    fn bfchar_multiple_entries() {
        let c = cmap(
            "beginbfchar\n\
             <41> <0041>\n\
             <42> <0042>\n\
             <43> <0043>\n\
             endbfchar\n"
        );
        assert_eq!(c.get(0x41), Some('A'));
        assert_eq!(c.get(0x42), Some('B'));
        assert_eq!(c.get(0x43), Some('C'));
        assert!(c.get(0x44).is_none());
    }

    #[test]
    fn bfrange_sequential_mapping() {
        // <41>..<5A> → U+0041..U+005A  (A..Z)
        let c = cmap("beginbfrange\n<41> <5A> <0041>\nendbfrange\n");
        assert_eq!(c.get(0x41), Some('A'));
        assert_eq!(c.get(0x5A), Some('Z'));
        assert_eq!(c.len(), 0x5A - 0x41 + 1);
    }

    #[test]
    fn bfrange_array_form() {
        let c = cmap("beginbfrange\n<79> <7A> [<4E2D> <6587>]\nendbfrange\n");
        assert_eq!(c.get(0x79), Some('中'));   // U+4E2D
        assert_eq!(c.get(0x7A), Some('文'));   // U+6587
    }

    #[test]
    fn count_prefix_on_begin_line() {
        // "5 beginbfchar" 是合法写法
        let c = cmap("5 beginbfchar\n<20> <0020>\nendbfchar\n");
        assert_eq!(c.get(0x20), Some(' '));
    }

    #[test]
    fn multiple_sections_merged() {
        let c = cmap(
            "beginbfchar\n<01> <0041>\nendbfchar\n\
             beginbfrange\n<10> <12> <0042>\nendbfrange\n"
        );
        assert_eq!(c.get(0x01), Some('A'));
        assert_eq!(c.get(0x10), Some('B'));
        assert_eq!(c.get(0x11), Some('C'));
        assert_eq!(c.get(0x12), Some('D'));
    }

    #[test]
    fn unknown_glyph_id_returns_none() {
        let c = cmap("beginbfchar\n<41> <0041>\nendbfchar\n");
        assert!(c.get(0xFF).is_none());
    }

    #[test]
    fn utf16_surrogate_pair() {
        // U+1F600 😀  UTF-16BE 代理对：D83D DE00
        let c = cmap("beginbfchar\n<01> <D83DDE00>\nendbfchar\n");
        assert_eq!(c.get(0x01), Some('😀'));
    }

    #[test]
    fn hex_token_extraction() {
        let tokens = hex_tokens("<DE> <6A21>");
        assert_eq!(tokens, vec![vec![0xDEu8], vec![0x6Au8, 0x21u8]]);
    }

    #[test]
    fn parse_hex_odd_length() {
        // 奇数位数前补 '0'：'F' → "0F" → [0x0F]
        assert_eq!(parse_hex("F"), Some(vec![0x0Fu8]));
    }
}
