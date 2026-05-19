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
    #[serde(default)]
    pub color: Option<[f32; 3]>,
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
        let (_, _, page_w, _) = source.get_page_media_box(page_idx)?;
        let page_margin = 36.0_f64; // 0.5 inch minimum margin

        let original_content = source
            .get_page_content_data(page_idx)
            .context("reading page content stream")?;
        let stripped = content_stream::strip_text_operators(&original_content);
        let graphics_only = stripped.graphics;
        let underline_ys = stripped.underline_ys;

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

        // Read annotation rects for underline reconstruction
        let annot_rects = read_annot_rects(&doc, page_id);
        let has_link_annots = !annot_rects.is_empty();

        // Detect column layout for wrap boundary calculation
        let column_starts = detect_column_starts(blocks, page_w as f64);

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

            // Use block width, capped by page margin
            let max_right = page_w as f64 - page_margin;
            let effective_width = block.width.min((max_right - block.x).max(0.0));

            text_ops.push_str(&content_stream::generate_text_ops(
                &clean_text,
                font_name,
                block.font_size as f32,
                x,
                block.y as f32,
                effective_width as f32,
                block.height as f32,
                block.color,
                if use_cjk { cjk_ctx.as_ref() } else { None },
            ));

            // Regenerate underline if this block overlaps with a link annotation
            if has_link_annots {
                let cjk_w: Option<&dyn Fn(char) -> f32> = if use_cjk {
                    let wt_ref = &width_table;
                    Some(&|c: char| *wt_ref.get(&c).unwrap_or(&1000.0))
                } else {
                    None
                };
                let text_width = content_stream::calculate_text_width(
                    &clean_text,
                    block.font_size as f32,
                    cjk_w,
                );
                if text_width > 5.0 && block_is_linked(block, &annot_rects) {
                    text_ops.push_str(&content_stream::generate_underline_ops(
                        x,
                        block.y as f32,
                        text_width,
                    ));
                }
            }
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

        // Step 6: Keep link annotations for navigation but hide their borders
        if let Ok(page_obj) = doc.get_object(page_id) {
            if let Object::Dictionary(dict) = page_obj {
                if let Ok(Object::Array(annots)) = dict.get(b"Annots") {
                    let annot_refs: Vec<_> = annots.iter().filter_map(|a| {
                        if let Object::Reference(r) = a { Some(*r) } else { None }
                    }).collect();
                    for annot_ref in annot_refs {
                        if let Ok(Object::Dictionary(annot_dict)) = doc.get_object_mut(annot_ref) {
                            annot_dict.set("Border", Object::Array(vec![
                                Object::Integer(0), Object::Integer(0), Object::Integer(0),
                            ]));
                        }
                    }
                }
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

/// Detect columns and their median right boundaries.
/// Returns sorted list of (column_start, median_right).
fn detect_column_starts(blocks: &[&TranslatedBlock], page_w: f64) -> Vec<(f64, f64)> {
    if blocks.is_empty() {
        return vec![];
    }

    // Cluster blocks by x position
    let threshold = page_w * 0.05;
    let mut clusters: Vec<(f64, Vec<f64>)> = vec![]; // (min_x, [right edges])

    let mut sorted_blocks: Vec<&&TranslatedBlock> = blocks.iter().collect();
    sorted_blocks.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));

    for block in sorted_blocks {
        let right = block.x + block.width;
        let matched = clusters.iter_mut().find(|(min_x, _)| (block.x - *min_x).abs() < threshold);
        if let Some(cluster) = matched {
            cluster.1.push(right);
        } else {
            clusters.push((block.x, vec![right]));
        }
    }

    clusters.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // For each cluster, compute median right edge (robust against word-spacing outliers)
    clusters.iter().map(|(min_x, rights)| {
        let mut sorted = rights.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = sorted[sorted.len() / 2];
        (*min_x, median)
    }).collect()
}

/// Find the right boundary for a block using column detection.
/// Uses: min(median_right_of_column, next_column_start - gap, page - margin)
fn find_column_right(block_x: f64, columns: &[(f64, f64)], page_w: f64) -> f64 {
    let margin = 36.0;
    let col_gap = 12.0;

    for i in 0..columns.len() {
        let (col_start, median_right) = columns[i];
        let next_col_start = columns.get(i + 1).map(|(x, _)| *x);

        if (block_x - col_start).abs() < page_w * 0.05
            || (block_x >= col_start && block_x < next_col_start.unwrap_or(page_w))
        {
            let boundary = match next_col_start {
                Some(next) => median_right.min(next - col_gap),
                None => median_right.min(page_w - margin),
            };
            return boundary;
        }
    }

    page_w - margin
}

/// Read link annotation Rects from a page [x0, y0, x1, y1]
fn read_annot_rects(doc: &lopdf::Document, page_id: lopdf::ObjectId) -> Vec<[f64; 4]> {
    use lopdf::Object;
    let mut rects = Vec::new();
    let page_dict = match doc.get_object(page_id) {
        Ok(Object::Dictionary(d)) => d,
        _ => return rects,
    };
    let annots = match page_dict.get(b"Annots") {
        Ok(Object::Array(arr)) => arr.clone(),
        _ => return rects,
    };
    for annot_ref in &annots {
        let annot_id = match annot_ref {
            Object::Reference(r) => *r,
            _ => continue,
        };
        if let Ok(Object::Dictionary(annot_dict)) = doc.get_object(annot_id) {
            if let Ok(Object::Array(rect)) = annot_dict.get(b"Rect") {
                if rect.len() == 4 {
                    let vals: Vec<f64> = rect.iter().map(|v| match v {
                        Object::Real(f) => *f as f64,
                        Object::Integer(i) => *i as f64,
                        _ => 0.0,
                    }).collect();
                    rects.push([vals[0], vals[1], vals[2], vals[3]]);
                }
            }
        }
    }
    rects
}

/// Check if a text block overlaps with any link annotation rect
fn block_is_linked(block: &TranslatedBlock, annot_rects: &[[f64; 4]]) -> bool {
    let bx = block.x;
    let by = block.y;
    // PDF annotation Rect is [x0, y0, x1, y1] (bottom-left to top-right)
    annot_rects.iter().any(|r| {
        bx >= r[0] - 5.0 && bx <= r[2] + 5.0 &&
        by >= r[1] - 5.0 && by <= r[3] + 5.0
    })
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
                color: None,
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
