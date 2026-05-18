import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const PLATFORM_PACKAGES: Record<string, string> = {
  "darwin-arm64": "@margin/pdf-translate-darwin-arm64",
  "darwin-x64": "@margin/pdf-translate-darwin-x64",
  "linux-x64": "@margin/pdf-translate-linux-x64-gnu",
  "win32-x64": "@margin/pdf-translate-win32-x64",
};

function getBinaryPath(): string {
  const platformKey = `${process.platform}-${process.arch}`;
  const pkg = PLATFORM_PACKAGES[platformKey];

  if (pkg) {
    try {
      const pkgDir = dirname(
        fileURLToPath(import.meta.resolve(`${pkg}/package.json`)),
      );
      const bin =
        process.platform === "win32"
          ? "pdf-translate-core.exe"
          : "pdf-translate-core";
      const binPath = join(pkgDir, bin);
      if (existsSync(binPath)) return binPath;
    } catch {
      // Platform package not installed, fall through
    }
  }

  // Fallback: check for local dev build
  const devBin = join(__dirname, "..", "..", "..", "target", "release", "pdf-translate-core");
  if (existsSync(devBin)) return devBin;

  const devBinDebug = join(__dirname, "..", "..", "..", "target", "debug", "pdf-translate-core");
  if (existsSync(devBinDebug)) return devBinDebug;

  throw new Error(
    `No pdf-translate-core binary found for ${platformKey}. ` +
      `Install the platform package: npm install ${pkg ?? "pdf-translate"}`,
  );
}

let cachedBinaryPath: string | undefined;

function binary(): string {
  if (!cachedBinaryPath) {
    cachedBinaryPath = getBinaryPath();
  }
  return cachedBinaryPath;
}

export interface TextBlock {
  page: number;
  text: string;
  x: number;
  y: number;
  width: number;
  height: number;
  font_size: number;
  font_name: string | null;
}

export interface PageInfo {
  page: number;
  width: number;
  height: number;
  blocks: TextBlock[];
}

export interface ExtractionResult {
  pages: PageInfo[];
  total_pages: number;
}

export function extractText(pdfPath: string): ExtractionResult {
  const output = execFileSync(binary(), ["extract", pdfPath], {
    encoding: "utf-8",
    maxBuffer: 100 * 1024 * 1024,
  });
  return JSON.parse(output);
}

export function extractHtml(pdfPath: string): string {
  return execFileSync(binary(), ["html", pdfPath], {
    encoding: "utf-8",
    maxBuffer: 100 * 1024 * 1024,
  });
}

export function overlayTranslations(
  inputPath: string,
  outputPath: string,
  translationsPath: string,
  inplace: boolean = true,
): void {
  const cmd = inplace ? "overlay-inplace" : "overlay";
  execFileSync(
    binary(),
    [cmd, inputPath, "--output", outputPath, "--translations", translationsPath],
    { encoding: "utf-8", maxBuffer: 100 * 1024 * 1024 },
  );
}
