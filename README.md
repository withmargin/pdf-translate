# pdf-translate

Translate PDF documents using LLMs. Supports OpenAI, Claude, Gemini, and any OpenAI-compatible API.

Preserves original PDF layout — backgrounds, images, shapes, and logos stay intact. Only text is replaced with translations.

## Install

```bash
npm install -g pdf-translate
```

Or run directly without installing:

```bash
npx pdf-translate input.pdf -l zh-TW
```

## Usage

```bash
# Translate to Traditional Chinese (default)
pdf-translate input.pdf -l zh-TW

# Translate to Japanese using Claude
pdf-translate input.pdf -l ja --provider claude

# Translate using Gemini
pdf-translate input.pdf -l ko --provider gemini --model gemini-2.5-flash

# Use a local model (Ollama, vLLM, etc.)
pdf-translate input.pdf -l zh-TW --base-url http://localhost:11434/v1 --model qwen3

# Translate specific pages
pdf-translate input.pdf -l zh-TW --pages 0-4

# Specify output path
pdf-translate input.pdf -l de --output translated.pdf

# List available models from a provider
pdf-translate models --provider claude
```

## Providers

| Provider | Flag | Environment Variable |
| -------- | ---- | ------------------- |
| OpenAI | `--provider openai` (default) | `OPENAI_API_KEY` |
| Claude | `--provider claude` | `ANTHROPIC_API_KEY` |
| Gemini | `--provider gemini` | `GEMINI_API_KEY` |
| Custom | `--base-url <url>` | `OPENAI_API_KEY` (optional) |

## How it works

1. **Extract** — pdf_oxide reads text spans with coordinates, fonts, and colors
2. **Translate** — LLM translates each page independently (page-by-page to prevent context bleeding)
3. **Replace** — Content stream manipulation: strip original text operators, inject translated text with CJK font embedding
4. **Output** — Modified PDF with original layout preserved

```
┌──────────────────────────────────────────────┐
│  Node.js CLI (TypeScript)                    │
│  - Command-line interface                    │
│  - LLM API calls (openai SDK)               │
│  - Per-page translation with batching        │
├──────────────────────────────────────────────┤
│  Rust Core (pdf_oxide + lopdf)               │
│  - PDF text extraction with coordinates      │
│  - Content stream manipulation               │
│  - CJK font embedding (Noto Sans CJK TC)    │
│  - Mixed CJK/Latin font rendering            │
└──────────────────────────────────────────────┘
```

## Known limitations (v0.1.0)

- **Multi-span lines**: Text split across multiple PDF spans on the same line may show gaps after translation. Paragraph grouping is planned for v0.2.0.
- **No text reflow**: Translated text longer than the original may overflow its bounding box.
- **CJK font size**: The embedded Noto Sans CJK TC font adds ~16MB to the output file. Font subsetting is planned.
- **First CJK run**: Downloads the CJK font (~16MB) on first use. Cached after that.

## Development

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) >= 22
- [pnpm](https://pnpm.io/) >= 10

### Setup

```bash
# Build Rust core
cargo build --release --manifest-path crates/core/Cargo.toml

# Install Node.js dependencies
pnpm install

# Build CLI
pnpm --filter pdf-translate build

# Run in development
pnpm --filter pdf-translate dev -- input.pdf -l zh-TW

# Run tests
cargo test --manifest-path crates/core/Cargo.toml --lib
```

## License

MIT
