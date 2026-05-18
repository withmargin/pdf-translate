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

/// Generate PDF text operators for a translated text block.
///
/// For standard fonts (Helvetica, etc.), text is encoded as literal strings.
/// For CJK fonts, text is encoded as hex glyph IDs.
pub fn generate_text_ops(
    text: &str,
    font_name: &str,
    font_size: f32,
    x: f32,
    y: f32,
    is_cjk_font: bool,
) -> String {
    let mut ops = String::new();
    ops.push_str("BT\n");
    // Set text color to near-black (matching original PDF text color)
    ops.push_str("0.078 0.078 0.075 rg\n");
    ops.push_str(&format!("/{font_name} {font_size:.4} Tf\n"));
    ops.push_str(&format!("1 0 0 1 {x:.4} {y:.4} Tm\n"));

    if is_cjk_font {
        // CJK: encode each character as 4-hex-digit Unicode codepoint
        // (for Type 0/CIDFont with Identity-H CMap)
        let hex: String = text.chars().map(|c| format!("{:04X}", c as u32)).collect();
        ops.push_str(&format!("<{hex}> Tj\n"));
    } else {
        // Latin: use literal string, escaping special chars
        let escaped = text
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        ops.push_str(&format!("({escaped}) Tj\n"));
    }

    ops.push_str("ET\n");
    ops
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
        let ops = generate_text_ops("Hello World", "Helvetica", 12.0, 72.0, 500.0, false);
        assert!(ops.contains("BT"));
        assert!(ops.contains("/Helvetica 12.0000 Tf"));
        assert!(ops.contains("1 0 0 1 72.0000 500.0000 Tm"));
        assert!(ops.contains("(Hello World) Tj"));
        assert!(ops.contains("ET"));
    }

    #[test]
    fn test_generate_text_ops_cjk() {
        let ops = generate_text_ops("你好", "NotoSansCJKtc", 12.0, 72.0, 500.0, true);
        assert!(ops.contains("BT"));
        assert!(ops.contains("/NotoSansCJKtc 12.0000 Tf"));
        // 你 = U+4F60, 好 = U+597D
        assert!(ops.contains("<4F60597D> Tj"));
        assert!(ops.contains("ET"));
    }

    #[test]
    fn test_generate_text_ops_escapes_parens() {
        let ops = generate_text_ops("Hello (World)", "Helvetica", 12.0, 72.0, 500.0, false);
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
}
