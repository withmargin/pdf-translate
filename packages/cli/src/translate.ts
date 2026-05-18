import type { ProviderConfig } from "./providers.js";
import { createClient } from "./providers.js";
import type { TextBlock, PageInfo } from "./bridge.js";

export interface TranslateOptions {
  provider: ProviderConfig;
  targetLang: string;
  sourceLang?: string;
}

const MAX_BLOCKS_PER_BATCH = 80;

export async function translatePages(
  pages: PageInfo[],
  options: TranslateOptions,
): Promise<string[]> {
  const client = createClient(options.provider);
  const allBlocks = pages.flatMap((p) => p.blocks);
  const results = new Array<string>(allBlocks.length).fill("");

  const batches = createBatches(allBlocks);
  let translated = 0;

  for (const batch of batches) {
    const batchTexts = batch.blocks.map((b) => b.text);
    const numberedText = batchTexts
      .map((text, i) => `[${i}] ${text}`)
      .join("\n");

    const sourceLangHint = options.sourceLang
      ? ` from ${options.sourceLang}`
      : "";

    const response = await client.chat.completions.create({
      model: options.provider.model,
      messages: [
        {
          role: "system",
          content: [
            `You are a professional translator. Translate the following numbered text segments${sourceLangHint} to ${options.targetLang}.`,
            "",
            "Rules:",
            "- Preserve the numbering format: [0] translated text",
            "- Translate each segment independently",
            "- Preserve any formatting, numbers, and proper nouns",
            "- Output ONLY the translated segments, nothing else",
          ].join("\n"),
        },
        {
          role: "user",
          content: numberedText,
        },
      ],
    });

    const content = response.choices[0]?.message?.content || "";
    const batchResults = parseNumberedResponse(content, batchTexts.length);

    for (let i = 0; i < batchResults.length; i++) {
      results[batch.startIndex + i] = batchResults[i];
    }

    translated += batch.blocks.length;
    process.stderr.write(
      `  Translated ${translated}/${allBlocks.length} blocks (batch ${batches.indexOf(batch) + 1}/${batches.length})\n`,
    );
  }

  return results;
}

interface Batch {
  startIndex: number;
  blocks: TextBlock[];
}

function createBatches(blocks: TextBlock[]): Batch[] {
  const batches: Batch[] = [];
  for (let i = 0; i < blocks.length; i += MAX_BLOCKS_PER_BATCH) {
    batches.push({
      startIndex: i,
      blocks: blocks.slice(i, i + MAX_BLOCKS_PER_BATCH),
    });
  }
  return batches;
}

function parseNumberedResponse(
  content: string,
  expectedCount: number,
): string[] {
  const results = new Array<string>(expectedCount).fill("");
  const lines = content.split("\n");

  for (const line of lines) {
    const match = line.match(/^\[(\d+)\]\s*(.+)/);
    if (match) {
      const index = parseInt(match[1], 10);
      if (index >= 0 && index < expectedCount) {
        results[index] = match[2].trim();
      }
    }
  }

  return results;
}
