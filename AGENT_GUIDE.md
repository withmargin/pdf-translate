# Agent Guide for pdf-translate

This guide helps AI agents use `pdf-translate` effectively.

## Quick start

```bash
# Check capabilities
pdf-translate describe

# Preview what will happen (no API calls)
pdf-translate input.pdf --dry-run --json

# Translate with structured output
pdf-translate input.pdf -l zh-TW --provider claude --json
```

## Rules

- Always use `--dry-run --json` before translating large PDFs
- Always use `--json` for programmatic workflows
- Use `--pages 0-4` to limit scope when testing
- Set API keys via environment variables, not `--api-key` flag
- Use relative file paths, not absolute

## Structured output

All commands support `--json` for machine-readable output.

### translate --dry-run --json

```json
{
  "status": "dry_run",
  "plan": {
    "input": "doc.pdf",
    "output": "doc.zh-TW.pdf",
    "provider": "claude",
    "model": "claude-sonnet-4-20250514",
    "totalPages": 36,
    "selectedPages": 5,
    "translatableBlocks": 41,
    "skippedBlocks": 12
  }
}
```

### translate --json

```json
{
  "status": "success",
  "output": "doc.zh-TW.pdf",
  "plan": { ... },
  "translated": 41
}
```

### Error format

```json
{
  "error": "FILE_NOT_FOUND",
  "message": "File not found: /path/to/missing.pdf"
}
```

Error codes: `FILE_NOT_FOUND`, `NO_TEXT`, `MODEL_LIST_ERROR`, `UNKNOWN_PROVIDER`.

## Environment variables

| Variable | Provider |
| --- | --- |
| `OPENAI_API_KEY` | OpenAI |
| `ANTHROPIC_API_KEY` | Claude |
| `GEMINI_API_KEY` | Gemini |
