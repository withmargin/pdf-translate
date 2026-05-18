# pdf-translate

Translate PDF documents using LLMs. Supports OpenAI, Claude, Gemini, and any OpenAI-compatible API.

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

# Specify output path
pdf-translate input.pdf -l de --output translated.pdf
```

## Providers

| Provider | Flag | Environment Variable |
| -------- | ---- | ------------------- |
| OpenAI | `--provider openai` (default) | `OPENAI_API_KEY` |
| Claude | `--provider claude` | `ANTHROPIC_API_KEY` |
| Gemini | `--provider gemini` | `GEMINI_API_KEY` |
| Custom | `--base-url <url>` | `OPENAI_API_KEY` (optional) |

## Architecture

```
┌──────────────────────────────────────────────┐
│  Node.js CLI (TypeScript)                    │
│  - Command-line interface                    │
│  - LLM API calls (openai SDK)               │
│  - Translation chunking & prompt strategy    │
├──────────────────────────────────────────────┤
│  Rust Core (pdf_oxide)                       │
│  - PDF text extraction with coordinates      │
│  - PDF overlay (translated text)             │
│  - CJK font embedding                       │
└──────────────────────────────────────────────┘
```

Platform-specific Rust binaries are distributed via npm optional dependencies, following the same pattern as [rolldown](https://github.com/nicolo-ribaudo/rolldown), [esbuild](https://github.com/nicolo-ribaudo/esbuild), and [SWC](https://github.com/nicolo-ribaudo/swc). Only the binary for your current platform is downloaded.

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
```

## License

MIT
