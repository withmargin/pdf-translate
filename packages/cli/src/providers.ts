import OpenAI from "openai";

export interface ProviderConfig {
  apiKey: string;
  baseUrl: string;
  model: string;
  defaultHeaders?: Record<string, string>;
}

export interface ProviderDef {
  envKey: string;
  baseUrl: string;
  defaultModel: string;
  defaultHeaders?: (apiKey: string) => Record<string, string>;
}

export const KNOWN_PROVIDERS: Record<string, ProviderDef> = {
  openai: {
    envKey: "OPENAI_API_KEY",
    baseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-4o",
  },
  claude: {
    envKey: "ANTHROPIC_API_KEY",
    baseUrl: "https://api.anthropic.com/v1/",
    defaultModel: "claude-sonnet-4-20250514",
    defaultHeaders: (apiKey) => ({
      "x-api-key": apiKey,
      "anthropic-version": "2023-06-01",
    }),
  },
  gemini: {
    envKey: "GEMINI_API_KEY",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai/",
    defaultModel: "gemini-2.5-flash",
  },
};

export function resolveProvider(opts: {
  provider?: string;
  model?: string;
  apiKey?: string;
  baseUrl?: string;
}): ProviderConfig {
  if (opts.baseUrl) {
    return {
      apiKey: opts.apiKey || process.env.OPENAI_API_KEY || "ollama",
      baseUrl: opts.baseUrl,
      model: opts.model || "default",
    };
  }

  const providerName = opts.provider || "openai";
  const known = KNOWN_PROVIDERS[providerName];

  if (!known) {
    throw new Error(
      `Unknown provider "${providerName}". Use --base-url for custom endpoints. ` +
        `Known providers: ${Object.keys(KNOWN_PROVIDERS).join(", ")}`,
    );
  }

  const apiKey = opts.apiKey || process.env[known.envKey];
  if (!apiKey) {
    const lines = [
      `No API key found for provider "${providerName}".`,
      "",
      "To get started, set one of these environment variables:",
      "",
      "  export OPENAI_API_KEY=sk-...          # OpenAI (default)",
      "  export ANTHROPIC_API_KEY=sk-ant-...   # Claude",
      "  export GEMINI_API_KEY=...             # Gemini",
      "",
      "Or pass directly:",
      `  pdf-translate input.pdf --api-key <key> --provider ${providerName}`,
      "",
      "Or use a local model (no key needed):",
      "  pdf-translate input.pdf --base-url http://localhost:11434/v1 --model qwen3",
    ];
    throw new Error(lines.join("\n"));
  }

  return {
    apiKey,
    baseUrl: known.baseUrl,
    model: opts.model || known.defaultModel,
    defaultHeaders: known.defaultHeaders?.(apiKey),
  };
}

export function createClient(config: ProviderConfig): OpenAI {
  return new OpenAI({
    apiKey: config.apiKey,
    baseURL: config.baseUrl,
    defaultHeaders: config.defaultHeaders,
  });
}

export async function listModels(opts: {
  provider?: string;
  apiKey?: string;
  baseUrl?: string;
}): Promise<string[]> {
  const providerName = opts.provider || "openai";
  const known = KNOWN_PROVIDERS[providerName];

  const apiKey = opts.apiKey || (known && process.env[known.envKey]);
  const baseUrl = opts.baseUrl || known?.baseUrl;

  if (!baseUrl || !apiKey) {
    throw new Error(
      `Cannot list models: missing API key or base URL for "${providerName}".`,
    );
  }

  const config = resolveProvider({
    provider: providerName,
    apiKey,
    baseUrl: opts.baseUrl,
  });
  const client = createClient(config);

  try {
    const response = await client.models.list();
    const models: string[] = [];
    for await (const model of response) {
      models.push(model.id);
    }
    return models.sort();
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    throw new Error(
      `Failed to list models from ${providerName}: ${message}`,
    );
  }
}
