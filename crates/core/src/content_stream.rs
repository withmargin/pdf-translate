/// Strip all text-rendering operators from a PDF content stream,
/// preserving graphics, color, and marked content structure.
///
/// PDF text operators start with T (Tf, Tm, Tj, TJ, Td, T*, Tc, Tw, etc.)
/// plus BT/ET delimiters and the quote operators (' and ").
/// We also strip inline image operators (BI/ID/EI) as PDFMathTranslate does.
pub fn strip_text_operators(content: &[u8]) -> Vec<u8> {
    let input = String::from_utf8_lossy(content);
    let mut output = Vec::new();
    let mut in_bt_block = false;

    for line in input.lines() {
        let trimmed = line.trim();

        if trimmed == "BT" {
            in_bt_block = true;
            continue;
        }
        if trimmed == "ET" {
            in_bt_block = false;
            continue;
        }

        if in_bt_block {
            continue;
        }

        // Outside BT/ET, keep everything (graphics, marked content, etc.)
        output.extend_from_slice(line.as_bytes());
        output.push(b'\n');
    }

    output
}

/// Context for CJK text generation with glyph lookup and width calculation.
pub struct CjkTextContext {
    pub glyph_lookup: Box<dyn Fn(char) -> u16>,
    pub cjk_char_width: Box<dyn Fn(char) -> f32>,
    pub glyph_map: std::cell::RefCell<Vec<(u16, char)>>,
}

impl CjkTextContext {
    pub fn new(
        glyph_lookup: Box<dyn Fn(char) -> u16>,
        cjk_char_width: Box<dyn Fn(char) -> f32>,
    ) -> Self {
        Self {
            glyph_lookup,
            cjk_char_width,
            glyph_map: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn into_glyph_map(self) -> Vec<(u16, char)> {
        self.glyph_map.into_inner()
    }
}

/// Generate PDF text operators for a translated text block.
/// When `cjk_ctx` is provided, splits text into CJK and Latin
/// segments, rendering each with the appropriate font.
pub fn generate_text_ops(
    text: &str,
    font_name: &str,
    font_size: f32,
    x: f32,
    y: f32,
    cjk_ctx: Option<&CjkTextContext>,
) -> String {
    if cjk_ctx.is_none() {
        return generate_latin_text_ops(text, font_name, font_size, x, y);
    }

    let ctx = cjk_ctx.unwrap();
    let latin_font = "TransLatin";
    let segments = split_by_script(text);

    let mut ops = String::new();
    let mut cursor_x = x;

    for seg in &segments {
        ops.push_str("BT\n");
        ops.push_str("0.078 0.078 0.075 rg\n");

        if seg.is_cjk {
            ops.push_str(&format!("/{font_name} {font_size:.4} Tf\n"));
            ops.push_str(&format!("1 0 0 1 {cursor_x:.4} {y:.4} Tm\n"));
            let mut hex = String::new();
            for c in seg.text.chars() {
                let gid = (ctx.glyph_lookup)(c);
                hex.push_str(&format!("{:04X}", gid));
                ctx.glyph_map.borrow_mut().push((gid, c));
                cursor_x += (ctx.cjk_char_width)(c) / 1000.0 * font_size;
            }
            ops.push_str(&format!("<{hex}> Tj\n"));
        } else {
            ops.push_str(&format!("/{latin_font} {font_size:.4} Tf\n"));
            ops.push_str(&format!("1 0 0 1 {cursor_x:.4} {y:.4} Tm\n"));
            let escaped = seg.text
                .replace('\\', "\\\\")
                .replace('(', "\\(")
                .replace(')', "\\)");
            ops.push_str(&format!("({escaped}) Tj\n"));
            for c in seg.text.chars() {
                cursor_x += helvetica_char_width(c) / 1000.0 * font_size;
            }
        }

        ops.push_str("ET\n");
    }

    ops
}

fn generate_latin_text_ops(text: &str, font_name: &str, font_size: f32, x: f32, y: f32) -> String {
    let mut ops = String::new();
    ops.push_str("BT\n");
    ops.push_str("0.078 0.078 0.075 rg\n");
    ops.push_str(&format!("/{font_name} {font_size:.4} Tf\n"));
    ops.push_str(&format!("1 0 0 1 {x:.4} {y:.4} Tm\n"));

    let escaped = text
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    ops.push_str(&format!("({escaped}) Tj\n"));
    ops.push_str("ET\n");
    ops
}

struct TextSegment {
    text: String,
    is_cjk: bool,
}

fn split_by_script(text: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut current_is_cjk = false;

    for c in text.chars() {
        let is_cjk = is_cjk_char(c);

        if !current.is_empty() && is_cjk != current_is_cjk {
            segments.push(TextSegment {
                text: current.clone(),
                is_cjk: current_is_cjk,
            });
            current.clear();
        }

        current_is_cjk = is_cjk;
        current.push(c);
    }

    if !current.is_empty() {
        segments.push(TextSegment {
            text: current,
            is_cjk: current_is_cjk,
        });
    }

    segments
}

/// Helvetica glyph widths (1/1000 em) from the PDF spec / AFM data.
pub fn helvetica_char_width(c: char) -> f32 {
    match c {
        ' ' => 278.0,
        '!' => 278.0, '"' => 355.0, '#' => 556.0, '$' => 556.0,
        '%' => 889.0, '&' => 667.0, '\'' => 191.0, '(' => 333.0,
        ')' => 333.0, '*' => 389.0, '+' => 584.0, ',' => 278.0,
        '-' => 333.0, '.' => 278.0, '/' => 278.0,
        '0'..='9' => 556.0,
        ':' => 278.0, ';' => 278.0, '<' => 584.0, '=' => 584.0,
        '>' => 584.0, '?' => 556.0, '@' => 1015.0,
        'A' => 667.0, 'B' => 667.0, 'C' => 722.0, 'D' => 722.0,
        'E' => 667.0, 'F' => 611.0, 'G' => 778.0, 'H' => 722.0,
        'I' => 278.0, 'J' => 500.0, 'K' => 667.0, 'L' => 556.0,
        'M' => 833.0, 'N' => 722.0, 'O' => 778.0, 'P' => 667.0,
        'Q' => 778.0, 'R' => 722.0, 'S' => 667.0, 'T' => 611.0,
        'U' => 722.0, 'V' => 667.0, 'W' => 944.0, 'X' => 667.0,
        'Y' => 667.0, 'Z' => 611.0,
        '[' => 278.0, '\\' => 278.0, ']' => 278.0, '^' => 469.0,
        '_' => 556.0, '`' => 333.0,
        'a' => 556.0, 'b' => 556.0, 'c' => 500.0, 'd' => 556.0,
        'e' => 556.0, 'f' => 278.0, 'g' => 556.0, 'h' => 556.0,
        'i' => 222.0, 'j' => 222.0, 'k' => 500.0, 'l' => 222.0,
        'm' => 833.0, 'n' => 556.0, 'o' => 556.0, 'p' => 556.0,
        'q' => 556.0, 'r' => 333.0, 's' => 500.0, 't' => 278.0,
        'u' => 556.0, 'v' => 500.0, 'w' => 722.0, 'x' => 500.0,
        'y' => 500.0, 'z' => 500.0,
        '{' => 334.0, '|' => 260.0, '}' => 334.0, '~' => 584.0,
        _ => 556.0,
    }
}

fn is_cjk_char(c: char) -> bool {
    let cp = c as u32;
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0x3040..=0x30FF).contains(&cp)
        || (0xAC00..=0xD7AF).contains(&cp)
        || (0x3100..=0x312F).contains(&cp)
        || (0x3000..=0x303F).contains(&cp)
        || (0xFF00..=0xFFEF).contains(&cp)
}

/// Combine stripped graphics with new translated text into a complete content stream.
pub fn build_replaced_content(
    graphics_ops: &[u8],
    text_ops: &str,
) -> Vec<u8> {
    let mut content = Vec::new();

    // Save graphics state, render original graphics, restore state
    content.extend_from_slice(b"q\n");
    content.extend_from_slice(graphics_ops);
    content.extend_from_slice(b"Q\n");

    // Add translated text on top
    content.extend_from_slice(text_ops.as_bytes());

    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_text_simple() {
        let input = b"q\n0.5 0.5 0.5 rg\n100 200 300 400 re\nf\nQ\nBT\n/TT0 12 Tf\n(Hello) Tj\nET\n";
        let result = strip_text_operators(input);
        let text = String::from_utf8_lossy(&result);

        assert!(text.contains("100 200 300 400 re"));
        assert!(text.contains("0.5 0.5 0.5 rg"));
        assert!(!text.contains("Hello"));
        assert!(!text.contains("BT"));
        assert!(!text.contains("ET"));
        assert!(!text.contains("Tf"));
    }

    #[test]
    fn test_strip_preserves_graphics() {
        let input = b"q\n0.851 0.467 0.341 rg\n0 0 792 612 re\nf\nQ\nBT\n/TT0 1 Tf\n56 0 0 56 54 502 Tm\n[(Hello)] TJ\nET\n";
        let result = strip_text_operators(input);
        let text = String::from_utf8_lossy(&result);

        assert!(text.contains("0.851 0.467 0.341 rg"));
        assert!(text.contains("0 0 792 612 re"));
        assert!(!text.contains("56 0 0 56"));
        assert!(!text.contains("Hello"));
    }

    #[test]
    fn test_generate_text_ops_latin() {
        let ops = generate_text_ops("Hello World", "Helvetica", 12.0, 72.0, 500.0, None);
        assert!(ops.contains("BT"));
        assert!(ops.contains("/Helvetica 12.0000 Tf"));
        assert!(ops.contains("1 0 0 1 72.0000 500.0000 Tm"));
        assert!(ops.contains("(Hello World) Tj"));
        assert!(ops.contains("ET"));
    }

    #[test]
    fn test_generate_text_ops_with_cjk_context() {
        let ctx = CjkTextContext::new(
            Box::new(|c: char| (c as u16) + 100),
            Box::new(|_c: char| 1000.0),
        );
        let ops = generate_text_ops("你好", "CJKFont", 12.0, 72.0, 500.0, Some(&ctx));
        assert!(ops.contains("/CJKFont 12.0000 Tf"));
        assert!(ops.contains("<4FC459E1> Tj"));
        let map = ctx.into_glyph_map();
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_generate_text_ops_cjk_identity() {
        let ctx = CjkTextContext::new(
            Box::new(|c: char| c as u16),
            Box::new(|_c: char| 1000.0),
        );
        let ops = generate_text_ops("你好", "NotoSansCJKtc", 12.0, 72.0, 500.0, Some(&ctx));
        assert!(ops.contains("/NotoSansCJKtc 12.0000 Tf"));
        assert!(ops.contains("<4F60597D> Tj"));
    }

    #[test]
    fn test_generate_text_ops_escapes_parens() {
        let ops = generate_text_ops("Hello (World)", "Helvetica", 12.0, 72.0, 500.0, None);
        assert!(ops.contains("(Hello \\(World\\)) Tj"));
    }

    #[test]
    fn test_build_replaced_content() {
        let graphics = b"0.5 0.5 0.5 rg\n100 200 300 400 re\nf\n";
        let text = "BT\n/Helvetica 12 Tf\n(Hello) Tj\nET\n";
        let result = build_replaced_content(graphics, text);
        let content = String::from_utf8_lossy(&result);

        // Graphics wrapped in q/Q
        assert!(content.starts_with("q\n"));
        assert!(content.contains("Q\n"));
        // Text comes after
        assert!(content.contains("BT\n/Helvetica"));
    }

    #[test]
    fn test_strip_real_content_stream() {
        // Simulated real PDF content stream fragment
        let input = br#"/P <</Lang (en-US)/MCID 0 >>BDC
BT
0.078 0.078 0.075 rg
/TT0 1 Tf
56 0 0 56 54 502.4204 Tm
[(Th)10 (e Playbook)]TJ
ET
EMC
q
0 0 792 612 re
W n
0.078 0.078 0.075 RG
0.5 w 4 M
S
Q
"#;
        let result = strip_text_operators(input);
        let text = String::from_utf8_lossy(&result);

        // Marked content tags preserved
        assert!(text.contains("BDC"));
        assert!(text.contains("EMC"));
        // Graphics preserved
        assert!(text.contains("0 0 792 612 re"));
        assert!(text.contains("0.5 w 4 M"));
        // Text stripped
        assert!(!text.contains("Playbook"));
        assert!(!text.contains("56 0 0 56"));
    }

    #[test]
    fn test_split_by_script_pure_cjk() {
        let segs = split_by_script("你好世界");
        assert_eq!(segs.len(), 1);
        assert!(segs[0].is_cjk);
        assert_eq!(segs[0].text, "你好世界");
    }

    #[test]
    fn test_split_by_script_pure_latin() {
        let segs = split_by_script("Hello World");
        assert_eq!(segs.len(), 1);
        assert!(!segs[0].is_cjk);
    }

    #[test]
    fn test_split_by_script_mixed() {
        let segs = split_by_script("打造AI原生");
        assert_eq!(segs.len(), 3);
        assert!(segs[0].is_cjk);
        assert_eq!(segs[0].text, "打造");
        assert!(!segs[1].is_cjk);
        assert_eq!(segs[1].text, "AI");
        assert!(segs[2].is_cjk);
        assert_eq!(segs[2].text, "原生");
    }

    #[test]
    fn test_split_by_script_mixed_with_spaces() {
        let segs = split_by_script("打造 AI 原生");
        // "打造" (CJK), " AI " (Latin+spaces), "原生" (CJK)
        assert_eq!(segs.len(), 3);
        assert!(segs[0].is_cjk);
        assert_eq!(segs[0].text, "打造");
        assert!(!segs[1].is_cjk);
        assert_eq!(segs[1].text, " AI ");
        assert!(segs[2].is_cjk);
        assert_eq!(segs[2].text, "原生");
    }

    #[test]
    fn test_mixed_text_uses_both_fonts() {
        let ctx = CjkTextContext::new(
            Box::new(|c: char| c as u16),
            Box::new(|_c: char| 1000.0),
        );
        let ops = generate_text_ops("打造AI原生", "CJKFont", 12.0, 72.0, 500.0, Some(&ctx));
        assert!(ops.contains("/CJKFont"));
        assert!(ops.contains("/TransLatin"));
        assert!(ops.contains("打造".chars().map(|c| format!("{:04X}", c as u16)).collect::<String>().as_str()));
        assert!(ops.contains("(AI)"));
    }

    // === Special character handling tests ===

    #[test]
    fn test_helvetica_digit_widths() {
        // All digits should have width 556
        for c in '0'..='9' {
            assert_eq!(helvetica_char_width(c), 556.0, "Digit '{c}' width");
        }
    }

    #[test]
    fn test_helvetica_narrow_vs_wide_chars() {
        // I, l, i should be narrow
        assert!(helvetica_char_width('I') < 300.0);
        assert!(helvetica_char_width('i') < 250.0);
        assert!(helvetica_char_width('l') < 250.0);
        // M, W should be wide
        assert!(helvetica_char_width('M') > 800.0);
        assert!(helvetica_char_width('W') > 900.0);
        // Space
        assert_eq!(helvetica_char_width(' '), 278.0);
    }

    #[test]
    fn test_strip_preserves_marked_content() {
        // BDC/EMC (marked content) should be preserved even though
        // they appear near text blocks
        let input = b"/P <</MCID 0 >>BDC\nBT\n/TT0 12 Tf\n(text) Tj\nET\nEMC\n";
        let result = strip_text_operators(input);
        let text = String::from_utf8_lossy(&result);
        assert!(text.contains("BDC"));
        assert!(text.contains("EMC"));
        assert!(!text.contains("text"));
    }

    #[test]
    fn test_strip_handles_multiple_bt_et_blocks() {
        let input = b"q\nBT\n(first) Tj\nET\n0.5 g\nBT\n(second) Tj\nET\nQ\n";
        let result = strip_text_operators(input);
        let text = String::from_utf8_lossy(&result);
        assert!(!text.contains("first"));
        assert!(!text.contains("second"));
        assert!(text.contains("0.5 g"));
        assert!(text.contains("q"));
        assert!(text.contains("Q"));
    }

    #[test]
    fn test_strip_handles_color_ops_outside_bt() {
        // Color operations (rg, RG, g, G) outside BT/ET should be preserved
        let input = b"0.85 0.47 0.34 rg\n0 0 100 100 re\nf\nBT\n0.08 0.08 0.07 rg\n/F1 12 Tf\n(text) Tj\nET\n";
        let result = strip_text_operators(input);
        let text = String::from_utf8_lossy(&result);
        // Background color preserved
        assert!(text.contains("0.85 0.47 0.34 rg"));
        // Rectangle preserved
        assert!(text.contains("0 0 100 100 re"));
        // Text color inside BT/ET stripped
        assert!(!text.contains("0.08 0.08 0.07 rg"));
        assert!(!text.contains("text"));
    }

    #[test]
    fn test_generate_text_ops_empty_string() {
        let ops = generate_text_ops("", "Helvetica", 12.0, 72.0, 500.0, None);
        assert!(ops.contains("() Tj"));
    }

    #[test]
    fn test_generate_text_ops_special_pdf_chars() {
        // Backslash and parentheses must be escaped in PDF literal strings
        let ops = generate_text_ops("a\\b(c)d", "Helvetica", 12.0, 72.0, 500.0, None);
        assert!(ops.contains("(a\\\\b\\(c\\)d) Tj"));
    }

    #[test]
    fn test_is_cjk_char_ranges() {
        // CJK Unified Ideographs
        assert!(is_cjk_char('你'));
        assert!(is_cjk_char('好'));
        // Hiragana
        assert!(is_cjk_char('あ'));
        // Katakana
        assert!(is_cjk_char('ア'));
        // Hangul
        assert!(is_cjk_char('한'));
        // Bopomofo
        assert!(is_cjk_char('ㄅ'));
        // CJK punctuation
        assert!(is_cjk_char('。'));
        assert!(is_cjk_char('，'));
        // Fullwidth forms
        assert!(is_cjk_char('Ａ'));
        // NOT CJK
        assert!(!is_cjk_char('A'));
        assert!(!is_cjk_char('1'));
        assert!(!is_cjk_char(' '));
        assert!(!is_cjk_char('é'));
    }

    #[test]
    fn test_split_by_script_punctuation() {
        // CJK punctuation should stay with CJK segments
        let segs = split_by_script("你好，世界！");
        assert_eq!(segs.len(), 1);
        assert!(segs[0].is_cjk);
    }

    #[test]
    fn test_split_by_script_numbers_with_cjk() {
        // Numbers are Latin, even between CJK chars
        let segs = split_by_script("第1章");
        assert_eq!(segs.len(), 3);
        assert!(segs[0].is_cjk);  // 第
        assert!(!segs[1].is_cjk); // 1
        assert!(segs[2].is_cjk);  // 章
    }

    #[test]
    fn test_split_by_script_arrows() {
        // "驗證" is CJK, " → " is Latin, "募資" is CJK
        let segs = split_by_script("驗證 → 募資");
        assert_eq!(segs.len(), 3);
        assert!(segs[0].is_cjk);   // 驗證
        assert!(!segs[1].is_cjk);  // " → "
        assert!(segs[2].is_cjk);   // 募資
    }
}
