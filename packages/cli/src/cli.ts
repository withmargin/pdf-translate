#!/usr/bin/env node

import { Command } from "commander";
import { writeFileSync, unlinkSync, existsSync } from "node:fs";
import { resolve, basename, dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { extractText, extractHtml, overlayTranslations } from "./bridge.js";
import {
  resolveProvider,
  listModels,
  KNOWN_PROVIDERS,
} from "./providers.js";
import { translatePages } from "./translate.js";
import type { TextBlock } from "./bridge.js";

const VERSION = "0.2.2";

function jsonOut(data: unknown) {
  console.log(JSON.stringify(data, null, 2));
}

function fail(code: string, message: string, detail?: Record<string, unknown>): never {
  const err = { error: code, message, ...detail };
  console.error(JSON.stringify(err));
  process.exit(1);
}

function shouldTranslate(text: string): boolean {
  const trimmed = text.replace(/\t/g, "").trim();
  if (!trimmed) return false;
  if (/^\d+$/.test(trimmed)) return false;
  return true;
}

function parsePageRange(
  range: string | undefined,
  totalPages: number,
): { start: number; end: number } | undefined {
  if (!range) return undefined;
  const [s, e] = range.split("-").map(Number);
  return { start: s, end: e ?? s };
}

const program = new Command();

program
  .name("pdf-translate")
  .description("Translate PDF documents using LLMs")
  .version(VERSION);

// ── translate (default command) ──────────────────────────────────

program
  .command("translate", { isDefault: true })
  .description("Translate a PDF file")
  .argument("<input>", "Path to the PDF file to translate")
  .option("-l, --lang <language>", "Target language", "zh-TW")
  .option("-s, --source-lang <language>", "Source language (auto-detect if omitted)")
  .option("-o, --output <path>", "Output file path")
  .option("-p, --provider <name>", "LLM provider: openai, claude, gemini (env: OPENAI_API_KEY, ANTHROPIC_API_KEY, GEMINI_API_KEY)", "openai")
  .option("-m, --model <model>", "Model name (uses provider default if omitted)")
  .option("--api-key <key>", "API key (or set via environment variable)")
  .option("--base-url <url>", "Custom OpenAI-compatible API endpoint (for Ollama, vLLM, etc.)")
  .option("--pages <range>", "Page range to translate (e.g. '0-4' for first 5 pages)")
  .option("--json", "Output structured JSON instead of human-readable text")
  .option("--dry-run", "Preview translation plan without calling the LLM")
  .action(async (input: string, opts) => {
    const inputPath = resolve(input);
    if (!existsSync(inputPath)) {
      fail("FILE_NOT_FOUND", `File not found: ${inputPath}`);
    }
    if (!inputPath.toLowerCase().endsWith(".pdf")) {
      fail("INVALID_FILE_TYPE", `Expected a PDF file, got: ${inputPath}`);
    }

    const outputPath =
      opts.output ||
      join(dirname(inputPath), `${basename(inputPath, ".pdf")}.${opts.lang}.pdf`);

    const providerName = opts.provider || "openai";
    const known = KNOWN_PROVIDERS[providerName];
    const modelName = opts.model || known?.defaultModel || "unknown";

    let provider;
    try {
      provider = opts.dryRun
        ? { apiKey: "", baseUrl: "", model: modelName }
        : resolveProvider({
            provider: opts.provider,
            model: opts.model,
            apiKey: opts.apiKey,
            baseUrl: opts.baseUrl,
          });
    } catch (e) {
      if (opts.json) {
        fail("NO_API_KEY", e instanceof Error ? e.message : String(e));
      }
      console.error(e instanceof Error ? e.message : String(e));
      process.exit(1);
    }

    // Extract
    let extraction;
    try {
      extraction = extractText(inputPath);
    } catch {
      fail("INVALID_PDF", `Failed to parse PDF. The file may be corrupted or not a valid PDF: ${inputPath}`);
    }

    let pagesToTranslate = extraction.pages;
    const pageRange = parsePageRange(opts.pages, extraction.total_pages);
    if (pageRange) {
      if (pageRange.start >= extraction.total_pages) {
        fail("PAGE_OUT_OF_RANGE", `Page range ${opts.pages} is outside document (${extraction.total_pages} pages, 0-indexed).`, {
          totalPages: extraction.total_pages,
          requestedRange: pageRange,
        });
      }
      pagesToTranslate = extraction.pages.filter(
        (p) => p.page >= pageRange.start && p.page <= pageRange.end,
      );
    }

    const allBlocks = pagesToTranslate.flatMap((p) => p.blocks);
    const translatableBlocks = allBlocks.filter((b) => shouldTranslate(b.text));
    const skipCount = allBlocks.length - translatableBlocks.length;

    const plan = {
      input: inputPath,
      output: outputPath,
      provider: opts.provider || "openai",
      model: provider.model,
      sourceLang: opts.sourceLang || "auto",
      targetLang: opts.lang,
      totalPages: extraction.total_pages,
      selectedPages: pagesToTranslate.length,
      totalBlocks: allBlocks.length,
      translatableBlocks: translatableBlocks.length,
      skippedBlocks: skipCount,
      pageRange: pageRange || { start: 0, end: extraction.total_pages - 1 },
    };

    // ── dry-run: return plan without translating ──
    if (opts.dryRun) {
      if (opts.json) {
        jsonOut({ status: "dry_run", plan });
      } else {
        console.log("Dry run — no API calls will be made.\n");
        console.log(`Input:       ${plan.input}`);
        console.log(`Output:      ${plan.output}`);
        console.log(`Provider:    ${plan.provider} (${plan.model})`);
        console.log(`Language:    ${plan.sourceLang} → ${plan.targetLang}`);
        console.log(`Pages:       ${plan.selectedPages}/${plan.totalPages}`);
        console.log(`Blocks:      ${plan.translatableBlocks} translatable, ${plan.skippedBlocks} skipped`);
      }
      return;
    }

    // ── human-readable progress ──
    if (!opts.json) {
      console.log(`Provider:  ${plan.provider} (${plan.model})`);
      console.log(`Input:     ${plan.input}`);
      console.log(`Output:    ${plan.output}`);
      console.log(`Language:  ${plan.sourceLang} → ${plan.targetLang}`);
      if (opts.pages) console.log(`Pages:     ${opts.pages}`);
      console.log();
      console.log(`Extracting: ${plan.totalPages} pages, ${plan.totalBlocks} blocks`);
      console.log(`Translating ${plan.translatableBlocks} blocks (skipping ${plan.skippedBlocks})...`);
    }

    if (plan.translatableBlocks === 0) {
      fail("NO_TEXT", "No translatable text found in selected pages.", {
        totalBlocks: plan.totalBlocks,
        skippedBlocks: plan.skippedBlocks,
      });
    }

    // ── translate ──
    const translatablePages = pagesToTranslate.map((p) => ({
      ...p,
      blocks: p.blocks.filter((b) => shouldTranslate(b.text)),
    }));

    const translations = await translatePages(translatablePages, {
      provider,
      targetLang: opts.lang,
      sourceLang: opts.sourceLang,
    });

    let tIdx = 0;
    const mergedTranslations = allBlocks.map((block) => {
      if (shouldTranslate(block.text)) {
        return translations[tIdx++] || block.text;
      }
      return block.text;
    });

    // ── write PDF ──
    const overlayData = {
      blocks: allBlocks.map((block, i) => ({
        page: block.page,
        original_text: block.text,
        text: mergedTranslations[i],
        x: block.x,
        y: block.y,
        width: block.width,
        height: block.height,
        font_size: block.font_size,
      })),
    };

    const tmpFile = join(tmpdir(), `pdf-translate-${Date.now()}.json`);
    writeFileSync(tmpFile, JSON.stringify(overlayData));
    try {
      overlayTranslations(inputPath, outputPath, tmpFile);
    } finally {
      try { unlinkSync(tmpFile); } catch {}
    }

    if (opts.json) {
      jsonOut({
        status: "success",
        output: outputPath,
        plan,
        translated: plan.translatableBlocks,
      });
    } else {
      console.log(`\nDone! Translated PDF saved to: ${outputPath}`);
    }
  });

// ── describe ─────────────────────────────────────────────────────

program
  .command("describe")
  .description("Describe supported capabilities and parameters (for agents)")
  .action(() => {
    jsonOut({
      name: "pdf-translate",
      version: VERSION,
      description: "Translate PDF documents using LLMs with layout preservation",
      commands: {
        translate: {
          description: "Translate a PDF file",
          args: { input: { type: "string", required: true, description: "Path to PDF file" } },
          options: {
            lang: { type: "string", default: "zh-TW", description: "Target language code" },
            sourceLang: { type: "string", description: "Source language (auto-detect if omitted)" },
            output: { type: "string", description: "Output file path" },
            provider: { type: "string", enum: Object.keys(KNOWN_PROVIDERS), default: "openai" },
            model: { type: "string", description: "Model name" },
            apiKey: { type: "string", description: "API key (prefer env vars instead)" },
            baseUrl: { type: "string", description: "Custom OpenAI-compatible endpoint (Ollama, vLLM)" },
            pages: { type: "string", description: "Page range, 0-indexed (e.g. '0-4' for first 5 pages)" },
            json: { type: "boolean", description: "Structured JSON output" },
            dryRun: { type: "boolean", description: "Preview plan without API calls (no key needed)" },
          },
        },
        models: { description: "List available models from a provider" },
        describe: { description: "Describe capabilities (this command)" },
      },
      providers: Object.fromEntries(
        Object.entries(KNOWN_PROVIDERS).map(([k, v]) => [
          k,
          { defaultModel: v.defaultModel, envKey: v.envKey },
        ]),
      ),
      limitations: [
        "Multi-span lines may show gaps after translation",
        "No text reflow for longer translations",
        "CJK font adds ~16MB to output",
      ],
    });
  });

// ── models ───────────────────────────────────────────────────────

program
  .command("models")
  .description("List available models from a provider")
  .option("-p, --provider <name>", "LLM provider", "openai")
  .option("--api-key <key>", "API key")
  .option("--base-url <url>", "Custom API endpoint")
  .option("--json", "Output as JSON array")
  .action(async (opts) => {
    try {
      const models = await listModels({
        provider: opts.provider,
        apiKey: opts.apiKey,
        baseUrl: opts.baseUrl,
      });

      if (opts.json) {
        const providerName = opts.provider || "openai";
        const known = KNOWN_PROVIDERS[providerName];
        jsonOut({
          provider: providerName,
          defaultModel: known?.defaultModel,
          models,
        });
      } else {
        const providerName = opts.provider || "openai";
        const known = KNOWN_PROVIDERS[providerName];
        const defaultModel = known?.defaultModel;
        console.log(`Models from ${providerName} (${models.length} available):\n`);
        for (const model of models) {
          const marker = model === defaultModel ? " (default)" : "";
          console.log(`  ${model}${marker}`);
        }
      }
    } catch (e) {
      fail("MODEL_LIST_ERROR", e instanceof Error ? e.message : String(e));
    }
  });

program.parse();
