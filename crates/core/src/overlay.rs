use anyhow::{Context, Result};
use pdf_oxide::document::PdfDocument;
use pdf_oxide::writer::{DocumentBuilder, EmbeddedFont, PageSize};
use serde::Deserialize;
use std::path::Path;

use crate::content_stream;
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

/// In-place text replacement: strip original text from content stream,
/// write translated text using the same page coordinates.
/// Preserves all non-text elements (images, backgrounds, shapes).
pub fn overlay_inplace(
    input_path: &Path,
    output_path: &Path,
    translations: &OverlayInput,
) -> Result<()> {
    use lopdf::{dictionary, Document, Object, Stream};
    use crate::pdf_font;

    let needs_cjk = translations
        .blocks
        .iter()
        .any(|b| fonts::text_needs_cjk(&b.text));

    let latin_font_name = "TransLatin";
    let cjk_font_name = "TransCJK";

    let source = PdfDocument::open(input_path)?;
    let page_count = source.page_count()?;

    let mut doc = Document::load(input_path).context("loading PDF with lopdf")?;

    // Register Latin font (Helvetica)
    let helvetica_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });

    let cjk_font_obj_id;
    let cjk_tounicode_id;
    // Pre-compute glyph ID and width lookup tables from the font
    let mut gid_table: std::collections::HashMap<char, u16> = std::collections::HashMap::new();
    let mut width_table: std::collections::HashMap<char, f32> = std::collections::HashMap::new();

    if needs_cjk {
        eprintln!("CJK text detected, embedding font...");
        let font_data = fonts::load_cjk_font_data()?;
        let (fid, tuid) = pdf_font::embed_cjk_font(&mut doc, cjk_font_name, &font_data);
        cjk_font_obj_id = Some(fid);
        cjk_tounicode_id = Some(tuid);

        let ef = pdf_oxide::writer::EmbeddedFont::from_data(
            Some(fonts::CJK_FONT_NAME.to_string()),
            font_data,
        )
        .map_err(|e| anyhow::anyhow!("Failed to parse CJK font: {e}"))?;

        // Pre-compute all unique CJK chars across all blocks
        for block in &translations.blocks {
            for c in block.text.chars() {
                if fonts::text_needs_cjk(&c.to_string()) {
                    gid_table.entry(c).or_insert_with(|| ef.glyph_id(c as u32).unwrap_or(0));
                    width_table.entry(c).or_insert_with(|| ef.char_width(c as u32) as f32);
                }
            }
        }
    } else {
        cjk_font_obj_id = None;
        cjk_tounicode_id = None;
    };

    // Group blocks by page
    let mut blocks_by_page: Vec<Vec<&TranslatedBlock>> = vec![vec![]; page_count];
    for block in &translations.blocks {
        if block.page < page_count {
            blocks_by_page[block.page].push(block);
        }
    }

    let page_ids: Vec<_> = doc.page_iter().collect();
    let mut all_glyph_mappings: Vec<(u16, char)> = Vec::new();

    for (page_idx, blocks) in blocks_by_page.iter().enumerate() {
        if blocks.is_empty() {
            continue;
        }

        let page_id = page_ids[page_idx];

        let original_content = source
            .get_page_content_data(page_idx)
            .context("reading page content stream")?;
        let graphics_only = content_stream::strip_text_operators(&original_content);

        let cjk_ctx = if needs_cjk {
            let gt = gid_table.clone();
            let wt = width_table.clone();
            Some(content_stream::CjkTextContext::new(
                Box::new(move |c: char| *gt.get(&c).unwrap_or(&0)),
                Box::new(move |c: char| *wt.get(&c).unwrap_or(&1000.0)),
            ))
        } else {
            None
        };

        let mut text_ops = String::new();
        for block in blocks {
            assert_eq!(block.page, page_idx, "Block page mismatch: block.page={} but processing page_idx={}", block.page, page_idx);

            // Strip tabs and right-align tab+number blocks within their bounding box
            let clean_text = block.text.replace('\t', "");
            let has_tab = block.text.contains('\t');
            let x = if has_tab && clean_text.trim().chars().all(|c| c.is_ascii_digit()) {
                // Right-align using actual Helvetica digit width (556/1000 em)
                let num_width: f64 = clean_text.trim().chars()
                    .map(|c| content_stream::helvetica_char_width(c) as f64 / 1000.0 * block.font_size)
                    .sum();
                (block.x + block.width - num_width) as f32
            } else {
                block.x as f32
            };

            let use_cjk = needs_cjk && fonts::text_needs_cjk(&clean_text);
            let font_name = if use_cjk { cjk_font_name } else { latin_font_name };

            text_ops.push_str(&content_stream::generate_text_ops(
                &clean_text,
                font_name,
                block.font_size as f32,
                x,
                block.y as f32,
                if use_cjk { cjk_ctx.as_ref() } else { None },
            ));
        }
        eprintln!("  Page {page_idx}: {} blocks, {} bytes of text ops", blocks.len(), text_ops.len());

        if let Some(ctx) = cjk_ctx {
            all_glyph_mappings.extend(ctx.into_glyph_map());
        }

        // Step 3: Combine and create new content stream object
        let new_content = content_stream::build_replaced_content(&graphics_only, &text_ops);
        let stream = Stream::new(dictionary! {}, new_content);
        let stream_id = doc.add_object(stream);

        // Step 4: Replace page's Contents reference
        if let Ok(page_obj) = doc.get_object_mut(page_id) {
            if let Object::Dictionary(dict) = page_obj {
                dict.set("Contents", Object::Reference(stream_id));
            }
        }

        // Step 5: Add font to page's Resources/Font dictionary
        if let Ok(page_obj) = doc.get_object_mut(page_id) {
            if let Object::Dictionary(dict) = page_obj {
                let resources = dict
                    .get_mut(b"Resources")
                    .ok()
                    .and_then(|r| r.as_dict_mut().ok());

                if let Some(resources) = resources {
                    let fonts = resources
                        .get_mut(b"Font")
                        .ok()
                        .and_then(|f| f.as_dict_mut().ok());

                    if let Some(fonts) = fonts {
                        fonts.set(latin_font_name, Object::Reference(helvetica_id));
                        if let Some(cjk_id) = cjk_font_obj_id {
                            fonts.set(cjk_font_name, Object::Reference(cjk_id));
                        }
                    } else {
                        let mut font_dict = dictionary! {
                            latin_font_name => Object::Reference(helvetica_id),
                        };
                        if let Some(cjk_id) = cjk_font_obj_id {
                            font_dict.set(cjk_font_name, Object::Reference(cjk_id));
                        }
                        resources.set("Font", font_dict);
                    }
                }
            }
        }

        // Step 6: Remove link annotations (they cause stale underlines)
        if let Ok(page_obj) = doc.get_object_mut(page_id) {
            if let Object::Dictionary(dict) = page_obj {
                dict.remove(b"Annots");
            }
        }

        eprintln!(
            "  Page {page_idx}: replaced content stream ({} blocks)",
            blocks.len()
        );
    }

    // Update ToUnicode CMap with actual glyph→Unicode mappings for copy/paste
    if let Some(tuid) = cjk_tounicode_id {
        pdf_font::update_tounicode(&mut doc, tuid, &all_glyph_mappings);
    }

    doc.save(output_path).context("saving PDF")?;

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
