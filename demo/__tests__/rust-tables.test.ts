import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { MCP_TOOLS, PROVIDERS } from "../backend/ai";
import { MAX_ATTACHED_NOTES, MISSING_CONVERSATION } from "../backend/commands/ai-chat";
import { NAME_IS_EMPTY } from "../backend/commands/buffers";

// The page answers from copies of Rust tables and refusals. These hold the
// copies to the source, so a provider, a tool or a sentence changed in Rust
// fails here rather than leaving the page describing an older app.
const ROOT = process.cwd();
const rust = (path: string) => readFileSync(join(ROOT, path), "utf8");

function fields(source: string, name: string): string[] {
  return [...source.matchAll(new RegExp(`\\b${name}: "([^"]*)"`, "g"))].map((match) => match[1]);
}

function list(source: string, constant: string): string[] {
  const body = source.slice(source.indexOf(`pub const ${constant}`));
  const array = body.slice(body.indexOf("["), body.indexOf("];") + 1);
  return [...array.matchAll(/"([^"]+)"/g)].map((match) => match[1]);
}

/** The `PROVIDERS` literal alone, so nothing in the file's tests is read as a row. */
function providerTable(source: string): string {
  const start = source.indexOf("pub const PROVIDERS");
  const end = source.indexOf("\n];", start);
  if (start === -1 || end === -1) throw new Error("providers.rs no longer declares PROVIDERS as a literal");
  return source.slice(start, end);
}

function stringConstant(source: string, name: string): string | undefined {
  return source.match(new RegExp(`const ${name}: &str = "([^"]*)";`))?.[1];
}

function numberConstant(source: string, name: string): number | undefined {
  const value = source.match(new RegExp(`const ${name}: usize = (\\d+);`))?.[1];
  return value === undefined ? undefined : Number(value);
}

describe("the demo's copies of Rust tables", () => {
  const providers = providerTable(rust("crates/writ-core/src/ai/providers.rs"));

  it("lists the providers writ_core::ai::providers lists, in its order", () => {
    expect(PROVIDERS.map((row) => row.id)).toEqual(fields(providers, "id"));
  });

  it("carries each provider's endpoint and default model", () => {
    const ids = fields(providers, "id");
    const urls = fields(providers, "base_url");
    const models = fields(providers, "default_model");
    expect(ids.length).toBeGreaterThan(0);
    expect(PROVIDERS.map((row) => row.base_url)).toEqual(urls);
    expect(PROVIDERS.map((row) => row.default_model)).toEqual(models);
  });

  it("carries each provider's suggested models and whether it needs a key", () => {
    const curated = [...providers.matchAll(/curated_models: &\[([^\]]*)\]/g)].map((match) =>
      [...match[1].matchAll(/"([^"]*)"/g)].map((model) => model[1]),
    );
    const needsKey = [...providers.matchAll(/needs_key: (true|false)/g)].map((match) => match[1] === "true");
    expect(curated).toHaveLength(PROVIDERS.length);
    expect(needsKey).toHaveLength(PROVIDERS.length);
    expect(PROVIDERS.map((row) => [...row.curated_models])).toEqual(curated);
    expect(PROVIDERS.map((row) => row.needs_key)).toEqual(needsKey);
  });

  it("offers the tools writ_core::tools offers", () => {
    const tools = rust("crates/writ-core/src/tools.rs");
    expect(MCP_TOOLS.read).toEqual(list(tools, "READ_TOOLS"));
    expect(MCP_TOOLS.write).toEqual(list(tools, "WRITE_TOOLS"));
  });

  it("refuses in the sentences the Rust commands use", () => {
    const notes = rust("crates/writ-core/src/notes/mod.rs");
    const chat = rust("src-tauri/src/commands/chat.rs");
    expect(stringConstant(notes, "NAME_IS_EMPTY")).toBe(NAME_IS_EMPTY);
    expect(stringConstant(chat, "MISSING_CONVERSATION")).toBe(MISSING_CONVERSATION);
    expect(numberConstant(chat, "MAX_ATTACHED_NOTES")).toBe(MAX_ATTACHED_NOTES);
  });
});
