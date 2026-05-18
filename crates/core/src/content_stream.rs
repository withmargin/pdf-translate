/// Strip all text-rendering operators from a PDF content stream,
/// preserving graphics, color, and marked content structure.
///
/// PDF text operators start with T (Tf, Tm, Tj, TJ, Td, T*, Tc, Tw, etc.)
/// plus BT/ET delimiters and the quote operators (' and ").
/// We also strip inline image operators (BI/ID/EI) as PDFMathTranslate does.
/// Result of stripping text from a content stream.
pub struct StrippedContent {
    pub graphics: Vec<u8>,
    /// Y positions (in content stream coords) where underlines were stripped.
    pub underline_ys: Vec<f32>,
}

pub fn strip_text_operators(content: &[u8]) -> StrippedContent {
    let input = String::from_utf8_lossy(content);
    let mut output = Vec::new();
    let mut underline_ys = Vec::new();
    let mut in_bt_block = false;
    let mut pending_rect: Option<(String, f32)> = None; // (line, y_position)

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

        if trimmed.ends_with(" re") {
            if is_underline_rect(trimmed) {
                let y = parse_rect_y(trimmed);
                pending_rect = Some((line.to_string(), y));
                continue;
            }
        }

        if let Some((ref rect_line, rect_y)) = pending_rect {
            if trimmed == "f" || trimmed == "f*" {
                underline_ys.push(rect_y);
                pending_rect = None;
                continue;
            } else {
                output.extend_from_slice(rect_line.as_bytes());
                output.push(b'\n');
                pending_rect = None;
            }
        }

        output.extend_from_slice(line.as_bytes());
        output.push(b'\n');
    }

    underline_ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    underline_ys.dedup_by(|a, b| (*a - *b).abs() < 2.0);

    StrippedContent {
        graphics: output,
        underline_ys,
    }
}

fn parse_rect_y(line: &str) -> f32 {
    let parts: Vec<&str> = line.trim().split_whitespace().collect();
    if parts.len() >= 2 {
        parts[1].parse().unwrap_or(0.0)
    } else {
        0.0
    }
}

fn is_underline_rect(line: &str) -> bool {
    let parts: Vec<&str> = line.trim().split_whitespace().collect();
    if parts.len() != 5 || parts[4] != "re" {
        return false;
    }
    let w: f32 = parts[2].parse().unwrap_or(0.0);
    let h: f32 = parts[3].parse().unwrap_or(0.0);
    // Thin rectangle (h ≤ 1.5) that isn't full-width (w < 500 = not a section rule)
    h.abs() <= 1.5 && w.abs() < 500.0 && w.abs() > 5.0
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
    color: Option<[f32; 3]>,
    cjk_ctx: Option<&CjkTextContext>,
) -> String {
    let [r, g, b] = color.unwrap_or([0.078, 0.078, 0.075]);
    let color_op = format!("{r:.4} {g:.4} {b:.4} rg\n");

    if cjk_ctx.is_none() {
        return generate_latin_text_ops(text, font_name, font_size, x, y, &color_op);
    }

    let ctx = cjk_ctx.unwrap();
    let latin_font = "TransLatin";
    let segments = split_by_script(text);

    let mut ops = String::new();
    let mut cursor_x = x;

    for seg in &segments {
        let has_non_ascii = seg.text.chars().any(|c| c as u32 > 127);

        if seg.is_cjk || has_non_ascii {
            // Render char-by-char: CJK font for chars with glyphs, Helvetica for fallback
            for c in seg.text.chars() {
                let gid = (ctx.glyph_lookup)(c);
                ops.push_str("BT\n");
                ops.push_str(&color_op);
                if gid != 0 {
                    ops.push_str(&format!("/{font_name} {font_size:.4} Tf\n"));
                    ops.push_str(&format!("1 0 0 1 {cursor_x:.4} {y:.4} Tm\n"));
                    ops.push_str(&format!("<{:04X}> Tj\n", gid));
                    ctx.glyph_map.borrow_mut().push((gid, c));
                    cursor_x += (ctx.cjk_char_width)(c) / 1000.0 * font_size;
                } else {
                    // Fallback to Helvetica with WinAnsiEncoding hex for non-ASCII
                    ops.push_str(&format!("/{latin_font} {font_size:.4} Tf\n"));
                    ops.push_str(&format!("1 0 0 1 {cursor_x:.4} {y:.4} Tm\n"));
                    if let Some(byte) = unicode_to_winansi(c) {
                        ops.push_str(&format!("<{:02X}> Tj\n", byte));
                    } else if c.is_ascii() {
                        let escaped = match c {
                            '\\' => "\\\\".to_string(),
                            '(' => "\\(".to_string(),
                            ')' => "\\)".to_string(),
                            _ => c.to_string(),
                        };
                        ops.push_str(&format!("({escaped}) Tj\n"));
                    } else {
                        // Last resort: skip unrenderable char
                        ops.push_str("( ) Tj\n");
                    }
                    cursor_x += helvetica_char_width(c) / 1000.0 * font_size;
                }
                ops.push_str("ET\n");
            }
            continue;
        } else {
            // Pure ASCII Latin — use Helvetica with literal string
            ops.push_str("BT\n");
            ops.push_str(&color_op);
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

fn generate_latin_text_ops(text: &str, font_name: &str, font_size: f32, x: f32, y: f32, color_op: &str) -> String {
    let mut ops = String::new();
    ops.push_str("BT\n");
    ops.push_str(color_op);
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

/// Calculate the rendered width of text in points, handling mixed CJK/Latin.
/// For CJK chars, uses the provided width lookup. For Latin, uses Helvetica metrics.
pub fn calculate_text_width(
    text: &str,
    font_size: f32,
    cjk_width_lookup: Option<&dyn Fn(char) -> f32>,
) -> f32 {
    let segments = split_by_script(text);
    let mut total = 0.0;

    for seg in &segments {
        if seg.is_cjk {
            if let Some(lookup) = cjk_width_lookup {
                for c in seg.text.chars() {
                    total += lookup(c) / 1000.0 * font_size;
                }
            } else {
                total += seg.text.chars().count() as f32 * font_size;
            }
        } else {
            for c in seg.text.chars() {
                total += helvetica_char_width(c) / 1000.0 * font_size;
            }
        }
    }

    total
}

/// Generate a thin underline rectangle matching the text width.
/// Draws at 2pt below the y baseline with height 0.75pt.
pub fn generate_underline_ops(x: f32, y: f32, width: f32) -> String {
    let underline_y = y - 2.0;
    format!("0.078 0.078 0.075 rg\n{x:.4} {underline_y:.4} {width:.4} 0.75 re\nf\n")
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

/// Map Unicode codepoints to WinAnsiEncoding bytes for Helvetica.
fn unicode_to_winansi(c: char) -> Option<u8> {
    match c {
        '\u{2026}' => Some(0x85), // …
        '\u{2018}' => Some(0x91), // '
        '\u{2019}' => Some(0x92), // '
        '\u{201C}' => Some(0x93), // "
        '\u{201D}' => Some(0x94), // "
        '\u{2022}' => Some(0x95), // •
        '\u{2013}' => Some(0x96), // –
        '\u{2014}' => Some(0x97), // —
        '\u{2122}' => Some(0x99), // ™
        '\u{00AB}' => Some(0xAB), // «
        '\u{00BB}' => Some(0xBB), // »
        '\u{00A9}' => Some(0xA9), // ©
        '\u{00AE}' => Some(0xAE), // ®
        '\u{00B0}' => Some(0xB0), // °
        '\u{00D7}' => Some(0xD7), // ×
        '\u{00F7}' => Some(0xF7), // ÷
        c if (c as u32) >= 0xA0 && (c as u32) <= 0xFF => Some(c as u8), // Latin-1 supplement
        _ => None,
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
        let stripped = strip_text_operators(input);
        let text = String::from_utf8_lossy(&stripped.graphics);

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
        let stripped = strip_text_operators(input);
        let text = String::from_utf8_lossy(&stripped.graphics);

        assert!(text.contains("0.851 0.467 0.341 rg"));
        assert!(text.contains("0 0 792 612 re"));
        assert!(!text.contains("56 0 0 56"));
        assert!(!text.contains("Hello"));
    }

    #[test]
    fn test_generate_text_ops_latin() {
        let ops = generate_text_ops("Hello World", "Helvetica", 12.0, 72.0, 500.0, None, None);
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
        let ops = generate_text_ops("你好", "CJKFont", 12.0, 72.0, 500.0, None, Some(&ctx));
        assert!(ops.contains("/CJKFont 12.0000 Tf"));
        // Per-char rendering: each char gets its own BT/ET
        assert!(ops.contains("<4FC4> Tj"));
        assert!(ops.contains("<59E1> Tj"));
        let map = ctx.into_glyph_map();
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_generate_text_ops_cjk_identity() {
        let ctx = CjkTextContext::new(
            Box::new(|c: char| c as u16),
            Box::new(|_c: char| 1000.0),
        );
        let ops = generate_text_ops("你好", "NotoSansCJKtc", 12.0, 72.0, 500.0, None, Some(&ctx));
        assert!(ops.contains("/NotoSansCJKtc 12.0000 Tf"));
        assert!(ops.contains("<4F60> Tj"));
        assert!(ops.contains("<597D> Tj"));
    }

    #[test]
    fn test_generate_text_ops_escapes_parens() {
        let ops = generate_text_ops("Hello (World)", "Helvetica", 12.0, 72.0, 500.0, None, None);
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
        let stripped = strip_text_operators(input);
        let text = String::from_utf8_lossy(&stripped.graphics);

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
        let ops = generate_text_ops("打造AI原生", "CJKFont", 12.0, 72.0, 500.0, None, Some(&ctx));
        // CJK chars use CJK font
        assert!(ops.contains("/CJKFont"));
        assert!(ops.contains(&format!("<{:04X}> Tj", '打' as u16)));
        // Latin chars use Helvetica
        assert!(ops.contains("/TransLatin"));
        assert!(ops.contains("(AI) Tj"));
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
        let stripped = strip_text_operators(input);
        let text = String::from_utf8_lossy(&stripped.graphics);
        assert!(text.contains("BDC"));
        assert!(text.contains("EMC"));
        assert!(!text.contains("text"));
    }

    #[test]
    fn test_strip_handles_multiple_bt_et_blocks() {
        let input = b"q\nBT\n(first) Tj\nET\n0.5 g\nBT\n(second) Tj\nET\nQ\n";
        let stripped = strip_text_operators(input);
        let text = String::from_utf8_lossy(&stripped.graphics);
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
        let stripped = strip_text_operators(input);
        let text = String::from_utf8_lossy(&stripped.graphics);
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
        let ops = generate_text_ops("", "Helvetica", 12.0, 72.0, 500.0, None, None);
        assert!(ops.contains("() Tj"));
    }

    #[test]
    fn test_generate_text_ops_special_pdf_chars() {
        // Backslash and parentheses must be escaped in PDF literal strings
        let ops = generate_text_ops("a\\b(c)d", "Helvetica", 12.0, 72.0, 500.0, None, None);
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

    // === Underline detection and stripping tests ===

    #[test]
    fn test_is_underline_rect() {
        // Typical underline: thin (h=1), moderate width
        assert!(is_underline_rect("60 1043 87.171875 1 re"));
        assert!(is_underline_rect("148 1115 420 1 re"));
        assert!(is_underline_rect("73 500 200 1.5 re"));
    }

    #[test]
    fn test_is_not_underline_section_rule() {
        // Full-width section dividers (w ≥ 500) should NOT be treated as underlines
        assert!(!is_underline_rect("60 969 622 1 re"));
        assert!(!is_underline_rect("0 0 792 1 re"));
    }

    #[test]
    fn test_is_not_underline_thick_rect() {
        // Rectangles with h > 1.5 are not underlines
        assert!(!is_underline_rect("60 500 200 5 re"));
        assert!(!is_underline_rect("60 500 200 20 re"));
    }

    #[test]
    fn test_is_not_underline_tiny_rect() {
        // Very small rects (w ≤ 5) are dots, not underlines
        assert!(!is_underline_rect("60 500 3 1 re"));
    }

    #[test]
    fn test_strip_removes_underline_rects() {
        let input = b"q\n0.5 g\n60 1043 87 1 re\nf\n100 200 300 400 re\nf\nQ\n";
        let stripped = strip_text_operators(input);
        let text = String::from_utf8_lossy(&stripped.graphics);
        // Underline rect (87x1) stripped
        assert!(!text.contains("60 1043 87 1 re"));
        // Normal rect (300x400) preserved
        assert!(text.contains("100 200 300 400 re"));
    }

    #[test]
    fn test_strip_preserves_section_rules() {
        let input = b"q\n60 969 622 1 re\nf\nQ\n";
        let stripped = strip_text_operators(input);
        let text = String::from_utf8_lossy(&stripped.graphics);
        // Full-width rule preserved
        assert!(text.contains("60 969 622 1 re"));
        assert!(text.contains("f"));
    }

    #[test]
    fn test_strip_underline_followed_by_non_fill() {
        // If a thin rect is NOT followed by "f", it should be preserved
        let input = b"60 1043 87 1 re\nS\n";
        let stripped = strip_text_operators(input);
        let text = String::from_utf8_lossy(&stripped.graphics);
        assert!(text.contains("60 1043 87 1 re"));
        assert!(text.contains("S"));
    }

    #[test]
    fn test_strip_tracks_underline_y_positions() {
        let input = b"60 1043 87 1 re\nf\n60 1079 88 1 re\nf\n60 969 622 1 re\nf\n";
        let stripped = strip_text_operators(input);
        // Two underlines stripped (y=1043, y=1079), section rule (y=969) preserved
        assert_eq!(stripped.underline_ys.len(), 2);
        assert!((stripped.underline_ys[0] - 1043.0).abs() < 0.1);
        assert!((stripped.underline_ys[1] - 1079.0).abs() < 0.1);
    }

    #[test]
    fn test_strip_deduplicates_nearby_underline_ys() {
        // Two underlines at nearly same y (same line, different x segments)
        let input = b"60 1043 87 1 re\nf\n147 1043 169 1 re\nf\n";
        let stripped = strip_text_operators(input);
        assert_eq!(stripped.underline_ys.len(), 1, "nearby ys should be deduped");
    }

    // === Text width calculation tests ===

    #[test]
    fn test_calculate_text_width_latin() {
        // "AI" at 12pt: A=667, I=278 → (667+278)/1000 * 12 = 11.34
        let w = calculate_text_width("AI", 12.0, None);
        assert!((w - 11.34).abs() < 0.1);
    }

    #[test]
    fn test_calculate_text_width_cjk() {
        // Each CJK char at 1000 width units, 12pt → 12pt per char
        let lookup = |_c: char| 1000.0_f32;
        let w = calculate_text_width("你好", 12.0, Some(&lookup));
        assert!((w - 24.0).abs() < 0.1);
    }

    #[test]
    fn test_calculate_text_width_mixed() {
        // "打造AI" at 12pt: 打(1000)+造(1000) as CJK, A(667)+I(278) as Latin
        let lookup = |_c: char| 1000.0_f32;
        let w = calculate_text_width("打造AI", 12.0, Some(&lookup));
        let expected = (1000.0 + 1000.0 + 667.0 + 278.0) / 1000.0 * 12.0;
        assert!((w - expected).abs() < 0.1);
    }

    #[test]
    fn test_generate_underline_ops() {
        let ops = generate_underline_ops(72.0, 500.0, 150.0);
        assert!(ops.contains("72.0000 498.0000 150.0000 0.75 re"));
        assert!(ops.contains("f"));
    }
}
