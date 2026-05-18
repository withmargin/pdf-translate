#!/usr/bin/env node

import { Command } from "commander";
import { writeFileSync, unlinkSync } from "node:fs";
import { resolve, basename, dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { extractText, overlayTranslations } from "./bridge.js";
import { resolveProvider, listModels, KNOWN_PROVIDERS } from "./providers.js";
import { translatePages } from "./translate.js";

const program = new Command();

program
  .name("pdf-translate")
  .description("Translate PDF documents using LLMs")
  .version("0.1.0");

program
  .command("translate", { isDefault: true })
  .description("Translate a PDF file")
  .argument("<input>", "Path to the PDF file to translate")
  .option("-l, --lang <language>", "Target language", "zh-TW")
  .option("-s, --source-lang <language>", "Source language (auto-detect if omitted)")
  .option("-o, --output <path>", "Output file path")
  .option("-p, --provider <name>", "LLM provider: openai, claude, gemini", "openai")
  .option("-m, --model <model>", "Model name (uses provider default if omitted)")
  .option("--api-key <key>", "API key (or set via environment variable)")
  .option("--base-url <url>", "Custom OpenAI-compatible API endpoint")
  .option("--pages <range>", "Page range to translate (e.g. '0-4' for first 5 pages)")
  .action(async (input: string, opts) => {
    const inputPath = resolve(input);
    const outputPath =
      opts.output ||
      join(
        dirname(inputPath),
        `${basename(inputPath, ".pdf")}.${opts.lang}.pdf`,
      );

    const provider = resolveProvider({
      provider: opts.provider,
      model: opts.model,
      apiKey: opts.apiKey,
      baseUrl: opts.baseUrl,
    });

    console.log(`Provider:  ${opts.provider || "openai"} (${provider.model})`);
    console.log(`Input:     ${inputPath}`);
    console.log(`Output:    ${outputPath}`);
    console.log(`Language:  ${opts.sourceLang || "auto"} → ${opts.lang}`);
    if (opts.pages) console.log(`Pages:     ${opts.pages}`);
    console.log();

    console.log("Extracting text from PDF...");
    const extraction = extractText(inputPath);

    // Filter pages if --pages specified
    let pagesToTranslate = extraction.pages;
    if (opts.pages) {
      const [start, end] = opts.pages.split("-").map(Number);
      pagesToTranslate = extraction.pages.filter(
        (p) => p.page >= start && p.page <= (end ?? start),
      );
    }

    const blockCount = pagesToTranslate.reduce(
      (sum, p) => sum + p.blocks.length,
      0,
    );
    console.log(`  Found ${extraction.total_pages} total pages, translating ${pagesToTranslate.length} pages (${blockCount} text blocks)`);

    if (blockCount === 0) {
      console.log("No text found in selected pages.");
      process.exit(1);
    }

    console.log("Translating...");
    const allBlocks = pagesToTranslate.flatMap((p) => p.blocks);
    const translations = await translatePages(pagesToTranslate, {
      provider,
      targetLang: opts.lang,
      sourceLang: opts.sourceLang,
    });

    console.log("Writing translated PDF...");
    const overlayData = {
      blocks: allBlocks.map((block, i) => ({
        page: block.page,
        original_text: block.text,
        text: translations[i] || block.text,
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
      try {
        unlinkSync(tmpFile);
      } catch {
        // ignore
      }
    }

    console.log();
    console.log(`Done! Translated PDF saved to: ${outputPath}`);
  });

program
  .command("models")
  .description("List available models from a provider")
  .option("-p, --provider <name>", "LLM provider: openai, claude, gemini", "openai")
  .option("--api-key <key>", "API key")
  .option("--base-url <url>", "Custom API endpoint")
  .action(async (opts) => {
    try {
      const models = await listModels({
        provider: opts.provider,
        apiKey: opts.apiKey,
        baseUrl: opts.baseUrl,
      });

      const providerName = opts.provider || "openai";
      const known = KNOWN_PROVIDERS[providerName];
      const defaultModel = known?.defaultModel;

      console.log(`Models from ${providerName} (${models.length} available):\n`);
      for (const model of models) {
        const marker = model === defaultModel ? " (default)" : "";
        console.log(`  ${model}${marker}`);
      }
    } catch (e) {
      console.error(e instanceof Error ? e.message : String(e));
      process.exit(1);
    }
  });

program.parse();
