import { defineConfig } from "rolldown";

export default defineConfig({
  input: "src/cli.ts",
  output: {
    dir: "dist",
    format: "esm",
    banner: "#!/usr/bin/env node",
  },
  platform: "node",
  external: [/^node:/, "openai"],
});
