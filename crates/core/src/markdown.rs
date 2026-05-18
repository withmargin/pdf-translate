use anyhow::Result;
use pdf_oxide::converters::ConversionOptions;
use pdf_oxide::document::PdfDocument;
use std::path::Path;

pub fn to_markdown(path: &Path) -> Result<String> {
    let doc = PdfDocument::open(path)?;
    let opts = ConversionOptions {
        detect_headings: true,
        extract_tables: true,
        ..Default::default()
    };
    let md = doc.to_markdown_all(&opts)?;
    Ok(md)
}
