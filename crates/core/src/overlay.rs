use anyhow::Result;
use pdf_oxide::writer::{DocumentBuilder, EmbeddedFont, PageSize};
use serde::Deserialize;
use std::path::Path;

use crate::fonts;

#[derive(Debug, Clone, Deserialize)]
pub struct TranslatedBlock {
    pub page: usize,
    #[serde(default)]
    pub original_text: Option<String>,
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub font_size: f64,
}

#[derive(Debug, Deserialize)]
pub struct OverlayInput {
    pub blocks: Vec<TranslatedBlock>,
    pub font_path: Option<String>,
}

/// Generate a new PDF with translated text, preserving page dimensions.
/// This is a "new pages" approach — original layout (images, backgrounds) is not preserved.
/// For layout-preserving translation, content stream manipulation is needed (Phase 2).
pub fn overlay_translations(
    input_path: &Path,
    output_path: &Path,
    translations: &OverlayInput,
) -> Result<()> {
    let needs_cjk = translations
        .blocks
        .iter()
        .any(|b| fonts::text_needs_cjk(&b.text));

    let cjk_font_data = if needs_cjk {
        eprintln!("CJK text detected, loading font...");
        Some(fonts::load_cjk_font_data()?)
    } else {
        None
    };

    let source = pdf_oxide::document::PdfDocument::open(input_path)?;
    let page_count = source.page_count()?;

    let mut builder = DocumentBuilder::new();

    if let Some(ref font_data) = cjk_font_data {
        let embedded = EmbeddedFont::from_data(
            Some(fonts::CJK_FONT_NAME.to_string()),
            font_data.clone(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to load CJK font: {e}"))?;
        builder = builder.register_embedded_font(fonts::CJK_FONT_NAME, embedded);
    }

    let mut blocks_by_page: Vec<Vec<&TranslatedBlock>> = vec![vec![]; page_count];
    for block in &translations.blocks {
        if block.page < page_count {
            blocks_by_page[block.page].push(block);
        }
    }

    for (page_idx, blocks) in blocks_by_page.iter().enumerate() {
        let (_, _, page_w, page_h) = source.get_page_media_box(page_idx)?;
        let mut pb = builder.page(PageSize::Custom(page_w, page_h));

        for block in blocks {
            let font_size = block.font_size as f32;
            let use_cjk = cjk_font_data.is_some() && fonts::text_needs_cjk(&block.text);
            let font_name = if use_cjk {
                fonts::CJK_FONT_NAME
            } else {
                "Helvetica"
            };

            pb = pb
                .font(font_name, font_size)
                .at(block.x as f32, block.y as f32)
                .text(&block.text);
        }

        pb.done();
    }

    let bytes = builder.build()?;
    std::fs::write(output_path, bytes)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_pdf_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.pdf")
    }

    #[test]
    fn test_overlay_nonexistent_file() {
        let input = OverlayInput {
            blocks: vec![],
            font_path: None,
        };
        let result = overlay_translations(
            Path::new("/nonexistent.pdf"),
            Path::new("/tmp/out.pdf"),
            &input,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_overlay_empty_blocks() {
        let path = sample_pdf_path();
        if !path.exists() {
            return;
        }

        let output =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/output_empty.pdf");
        let input = OverlayInput {
            blocks: vec![],
            font_path: None,
        };

        let result = overlay_translations(&path, &output, &input);
        assert!(result.is_ok());
        assert!(output.exists());

        let bytes = std::fs::read(&output).unwrap();
        assert!(bytes.starts_with(b"%PDF"));

        let _ = std::fs::remove_file(&output);
    }

    #[test]
    fn test_overlay_with_ascii_block() {
        let path = sample_pdf_path();
        if !path.exists() {
            return;
        }

        let output =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/output_test.pdf");
        let input = OverlayInput {
            blocks: vec![TranslatedBlock {
                page: 0,
                original_text: Some("Hello".to_string()),
                text: "Hello World".to_string(),
                x: 50.0,
                y: 700.0,
                width: 200.0,
                height: 20.0,
                font_size: 12.0,
            }],
            font_path: None,
        };

        let result = overlay_translations(&path, &output, &input);
        assert!(result.is_ok());

        let bytes = std::fs::read(&output).unwrap();
        assert!(bytes.starts_with(b"%PDF"));

        let _ = std::fs::remove_file(&output);
    }
}
