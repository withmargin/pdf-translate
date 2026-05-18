use anyhow::Result;
use pdf_oxide::document::PdfDocument;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct TextBlock {
    pub page: usize,
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub font_size: f64,
    pub font_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PageInfo {
    pub page: usize,
    pub width: f64,
    pub height: f64,
    pub blocks: Vec<TextBlock>,
}

#[derive(Debug, Serialize)]
pub struct ExtractionResult {
    pub pages: Vec<PageInfo>,
    pub total_pages: usize,
}

pub fn extract_text(path: &Path) -> Result<ExtractionResult> {
    let doc = PdfDocument::open(path)?;
    let page_count = doc.page_count()?;

    let mut pages = Vec::with_capacity(page_count);

    for page_idx in 0..page_count {
        // media_box returns (x, y, width, height)
        let (_, _, page_w, page_h) = doc.get_page_media_box(page_idx)?;
        let spans = doc.extract_spans(page_idx)?;

        let raw_blocks: Vec<TextBlock> = spans
            .into_iter()
            .filter(|span| !span.text.trim().is_empty())
            .map(|span| TextBlock {
                page: page_idx,
                text: span.text.clone(),
                x: span.bbox.x as f64,
                y: span.bbox.y as f64,
                width: span.bbox.width as f64,
                height: span.bbox.height as f64,
                font_size: span.font_size as f64,
                font_name: Some(span.font_name.clone()),
            })
            .collect();

        let blocks = merge_same_line_blocks(raw_blocks);

        pages.push(PageInfo {
            page: page_idx,
            width: page_w as f64,
            height: page_h as f64,
            blocks,
        });
    }

    Ok(ExtractionResult {
        total_pages: page_count,
        pages,
    })
}

/// Merge spans that are on the same line (same y ± threshold, same font size)
/// into single text blocks. Preserves the first span's x coordinate and
/// combines text with the bounding box spanning all merged spans.
fn merge_same_line_blocks(mut blocks: Vec<TextBlock>) -> Vec<TextBlock> {
    if blocks.is_empty() {
        return blocks;
    }

    // Sort by y (descending, PDF coords) then x (ascending)
    blocks.sort_by(|a, b| {
        let y_cmp = b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal);
        if y_cmp == std::cmp::Ordering::Equal {
            a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            y_cmp
        }
    });

    let mut merged: Vec<TextBlock> = Vec::new();
    let y_threshold = 1.0; // pt tolerance for "same line"

    for block in blocks {
        let should_merge = merged.last().map_or(false, |prev: &TextBlock| {
            let same_line = (prev.y - block.y).abs() < y_threshold;
            let same_size = (prev.font_size - block.font_size).abs() < 0.5;
            let same_page = prev.page == block.page;
            let prev_right = prev.x + prev.width;
            let x_gap = block.x - prev_right;
            let close_enough = x_gap < prev.font_size * 2.0;
            // Never merge tab-prefixed blocks (tab leaders for right-aligned content)
            let next_is_tab = block.text.starts_with('\t');
            same_line && same_size && same_page && close_enough && !next_is_tab
        });

        if should_merge {
            let prev = merged.last_mut().unwrap();
            prev.text.push_str(&block.text);
            // Expand bounding box to cover both spans
            let right = (prev.x + prev.width).max(block.x + block.width);
            prev.width = right - prev.x;
            if block.height > prev.height {
                prev.height = block.height;
            }
        } else {
            merged.push(block);
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_pdf_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.pdf")
    }

    #[test]
    fn test_extract_nonexistent_file() {
        let result = extract_text(Path::new("/nonexistent/file.pdf"));
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_sample_pdf() {
        let path = sample_pdf_path();
        if !path.exists() {
            eprintln!("Skipping test: sample.pdf not found at {}", path.display());
            return;
        }

        let result = extract_text(&path).expect("extraction should succeed");
        assert!(result.total_pages > 0);
        assert!(!result.pages.is_empty());

        for page in &result.pages {
            assert!(page.width > 0.0);
            assert!(page.height > 0.0);
        }
    }

    #[test]
    fn test_extract_has_text_blocks() {
        let path = sample_pdf_path();
        if !path.exists() {
            return;
        }

        let result = extract_text(&path).unwrap();
        let total_blocks: usize = result.pages.iter().map(|p| p.blocks.len()).sum();
        assert!(total_blocks > 0, "should find at least some text blocks");

        for block in result.pages.iter().flat_map(|p| &p.blocks) {
            assert!(!block.text.trim().is_empty());
            assert!(block.font_size > 0.0);
            assert!(block.width >= 0.0);
            assert!(block.height >= 0.0);
        }
    }

    fn make_block(page: usize, text: &str, x: f64, y: f64, w: f64, size: f64) -> TextBlock {
        TextBlock { page, text: text.to_string(), x, y, width: w, height: size, font_size: size, font_name: None }
    }

    #[test]
    fn test_merge_same_line_spans() {
        let blocks = vec![
            make_block(0, "The smar ter", 73.0, 594.1, 115.8, 22.5),
            make_block(0, ", faster", 187.7, 594.1, 66.2, 22.5),
            make_block(0, ", easier wa", 252.8, 594.1, 102.7, 22.5),
            make_block(0, "y to build a", 354.8, 594.1, 108.5, 22.5),
        ];
        let merged = merge_same_line_blocks(blocks);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "The smar ter, faster, easier way to build a");
    }

    #[test]
    fn test_merge_preserves_different_lines() {
        let blocks = vec![
            make_block(0, "Line one", 73.0, 600.0, 100.0, 12.0),
            make_block(0, "Line two", 73.0, 580.0, 100.0, 12.0),
        ];
        let merged = merge_same_line_blocks(blocks);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_merge_preserves_different_font_sizes() {
        let blocks = vec![
            make_block(0, "Title", 73.0, 600.0, 200.0, 24.0),
            make_block(0, "subtitle", 300.0, 600.0, 100.0, 12.0),
        ];
        let merged = merge_same_line_blocks(blocks);
        assert_eq!(merged.len(), 2, "different font sizes should not merge");
    }

    #[test]
    fn test_merge_chapter_split_word() {
        // "Chapter 5: What's Y" + "our Problem?" → merged
        let blocks = vec![
            make_block(0, "Chapter 5: What's Y", 73.0, 428.2, 121.1, 13.5),
            make_block(0, "our Problem?", 193.0, 428.2, 84.2, 13.5),
        ];
        let merged = merge_same_line_blocks(blocks);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "Chapter 5: What's Your Problem?");
        assert_eq!(merged[0].x, 73.0);
    }

    #[test]
    fn test_merge_different_pages_not_merged() {
        let blocks = vec![
            make_block(0, "Page 0 text", 73.0, 600.0, 100.0, 12.0),
            make_block(1, "Page 1 text", 73.0, 600.0, 100.0, 12.0),
        ];
        let merged = merge_same_line_blocks(blocks);
        assert_eq!(merged.len(), 2, "blocks from different pages should not merge");
    }

    #[test]
    fn test_merge_expands_bounding_box() {
        let blocks = vec![
            make_block(0, "Hello ", 50.0, 500.0, 60.0, 12.0),
            make_block(0, "World", 110.0, 500.0, 50.0, 12.0),
        ];
        let merged = merge_same_line_blocks(blocks);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].x, 50.0);
        assert!((merged[0].width - 110.0).abs() < 0.1); // 50+60=110 to 110+50=160, width=110
    }
}
