use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// Noto Sans CJK TC (Traditional Chinese) from Google's official noto-cjk GitHub releases.
/// Source: https://github.com/googlefonts/noto-cjk
/// License: SIL Open Font License 1.1
const FONT_URL: &str = "https://github.com/googlefonts/noto-cjk/raw/main/Sans/OTF/TraditionalChinese/NotoSansCJKtc-Regular.otf";
const FONT_FILENAME: &str = "NotoSansCJKtc-Regular.otf";

fn cache_dir() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("pdf-translate")
        .join("fonts");
    fs::create_dir_all(&dir).context("creating font cache directory")?;
    Ok(dir)
}

fn font_cache_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join(FONT_FILENAME))
}

pub fn ensure_cjk_font() -> Result<PathBuf> {
    let cached = font_cache_path()?;
    if cached.exists() {
        let metadata = fs::metadata(&cached)?;
        if metadata.len() > 1_000_000 {
            return Ok(cached);
        }
    }

    eprintln!("Downloading CJK font from Google Fonts (one-time)...");
    eprintln!("  Source: {FONT_URL}");
    eprintln!("  License: SIL Open Font License 1.1");

    let body = ureq::get(FONT_URL)
        .call()
        .context("downloading CJK font")?
        .into_body()
        .with_config()
        .limit(50_000_000)
        .read_to_vec()
        .context("reading font data")?;

    if body.len() < 1_000_000 {
        anyhow::bail!(
            "Downloaded font is too small ({} bytes), likely a redirect or error page",
            body.len()
        );
    }

    fs::write(&cached, &body).context("writing font to cache")?;
    eprintln!("  Cached to: {}", cached.display());

    Ok(cached)
}

pub fn load_cjk_font_data() -> Result<Vec<u8>> {
    let path = ensure_cjk_font()?;
    fs::read(&path).context("reading cached CJK font")
}

pub const CJK_FONT_NAME: &str = "NotoSansCJKtc";

pub fn text_needs_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        let cp = c as u32;
        (0x4E00..=0x9FFF).contains(&cp)
            || (0x3400..=0x4DBF).contains(&cp)
            || (0xF900..=0xFAFF).contains(&cp)
            || (0x3040..=0x30FF).contains(&cp)
            || (0xAC00..=0xD7AF).contains(&cp)
            || (0x3100..=0x312F).contains(&cp)
            || (0x3000..=0x303F).contains(&cp)
            || (0xFF00..=0xFFEF).contains(&cp)
    })
}
