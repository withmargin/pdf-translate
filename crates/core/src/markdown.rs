use anyhow::Result;
use pdf_oxide::converters::ConversionOptions;
use pdf_oxide::document::PdfDocument;
use std::path::Path;

/// Extract the background color of a page by parsing the first full-page
/// rectangle fill from the content stream.
fn extract_page_background(doc: &PdfDocument, page_idx: usize, page_w: f32, page_h: f32) -> String {
    let content = match doc.get_page_content_data(page_idx) {
        Ok(c) => c,
        Err(_) => return "#ffffff".to_string(),
    };
    let text = String::from_utf8_lossy(&content);

    let mut last_rg: Option<(f32, f32, f32)> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        // Track fill color: "R G B rg"
        if trimmed.ends_with(" rg") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() == 4 {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    parts[0].parse::<f32>(),
                    parts[1].parse::<f32>(),
                    parts[2].parse::<f32>(),
                ) {
                    last_rg = Some((r, g, b));
                }
            }
        }

        // Check for full-page rectangle fill: "x y w h re" followed by "f"
        if trimmed == "f" || trimmed == "f*" {
            if let Some((r, g, b)) = last_rg {
                return format!(
                    "#{:02x}{:02x}{:02x}",
                    (r * 255.0) as u8,
                    (g * 255.0) as u8,
                    (b * 255.0) as u8,
                );
            }
        }

        // Check for rectangle that covers most of the page
        if trimmed.ends_with(" re") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() == 5 {
                if let (Ok(w), Ok(h)) = (parts[2].parse::<f32>(), parts[3].parse::<f32>()) {
                    if w >= page_w * 0.9 && h >= page_h * 0.9 {
                        // This is a full-page rectangle, the fill will use last_rg
                        continue;
                    }
                }
            }
        }
    }

    "#ffffff".to_string()
}

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

pub fn to_html(path: &Path) -> Result<String> {
    let doc = PdfDocument::open(path)?;
    let page_count = doc.page_count()?;
    let opts = ConversionOptions {
        detect_headings: true,
        extract_tables: true,
        preserve_layout: true,
        include_images: true,
        embed_images: true,
        ..Default::default()
    };

    let mut pages_html = Vec::new();
    for page_idx in 0..page_count {
        let (_, _, w, h) = doc.get_page_media_box(page_idx)?;
        let page_html = doc.to_html(page_idx, &opts)?;
        let bg_color = extract_page_background(&doc, page_idx, w, h);
        pages_html.push(format!(
            r#"<div class="page" style="width:{w}pt;height:{h}pt;position:relative;overflow:hidden;background:{bg_color};">{page_html}</div>"#,
        ));
    }

    let full_html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ background: #e0e0e0; display: flex; flex-direction: column; align-items: center; gap: 16px; padding: 16px; font-family: -apple-system, "Noto Sans CJK TC", "Noto Sans CJK SC", sans-serif; }}
.page {{ box-shadow: 0 2px 8px rgba(0,0,0,0.15); position: relative; overflow: hidden; background: #fff; page-break-after: always; transform: scaleY(-1); }}
.page > div {{ transform: scaleY(-1); }}
@media print {{ body {{ background: none; gap: 0; padding: 0; }} .page {{ box-shadow: none; }} }}
</style>
</head>
<body>
{body}
</body>
</html>"#,
        body = pages_html.join("\n"),
    );

    Ok(full_html)
}
