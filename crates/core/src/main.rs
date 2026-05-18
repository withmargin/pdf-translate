use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use pdf_oxide::document::PdfDocument;
use pdf_translate_core::{extract, markdown, overlay};

#[derive(Parser)]
#[command(name = "pdf-translate-core", about = "PDF text extraction and overlay engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Extract text with coordinates from a PDF (JSON output)
    Extract {
        input: PathBuf,

        #[arg(short, long)]
        pages: Option<String>,
    },

    /// Extract PDF content as Markdown
    Markdown {
        input: PathBuf,
    },

    /// Dump raw page content stream (debug)
    DumpContent {
        input: PathBuf,
        #[arg(short, long, default_value = "0")]
        page: usize,
    },

    /// Overlay translated text onto a new PDF (new pages, no layout preservation)
    Overlay {
        input: PathBuf,

        #[arg(short, long)]
        output: PathBuf,

        #[arg(short, long)]
        translations: PathBuf,
    },

    /// In-place text replacement preserving original layout
    OverlayInplace {
        input: PathBuf,

        #[arg(short, long)]
        output: PathBuf,

        #[arg(short, long)]
        translations: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Extract { input, pages: _ } => {
            let result = extract::extract_text(&input)?;
            let json = serde_json::to_string_pretty(&result)?;
            println!("{json}");
        }
        Command::DumpContent { input, page } => {
            let doc = PdfDocument::open(&input)?;
            let content = doc.get_page_content_data(page)?;
            println!("{}", String::from_utf8_lossy(&content));
        }
        Command::Markdown { input } => {
            let md = markdown::to_markdown(&input)?;
            println!("{md}");
        }
        Command::Overlay {
            input,
            output,
            translations,
        } => {
            let data = std::fs::read_to_string(&translations)?;
            let input_data: overlay::OverlayInput = serde_json::from_str(&data)?;
            overlay::overlay_translations(&input, &output, &input_data)?;
            eprintln!("Translated PDF written to {}", output.display());
        }
        Command::OverlayInplace {
            input,
            output,
            translations,
        } => {
            let data = std::fs::read_to_string(&translations)?;
            let input_data: overlay::OverlayInput = serde_json::from_str(&data)?;
            overlay::overlay_inplace(&input, &output, &input_data)?;
            eprintln!("Translated PDF written to {}", output.display());
        }
    }

    Ok(())
}
