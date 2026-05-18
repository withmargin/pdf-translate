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

        let blocks: Vec<TextBlock> = spans
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
}
