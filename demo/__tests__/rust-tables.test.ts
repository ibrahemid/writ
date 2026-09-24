import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { MCP_TOOLS, PROVIDERS } from "../backend/ai";

// The page answers from copies of two Rust tables. These hold the copies to the
// source, so a provider or a tool added in Rust fails here rather than leaving
// the page describing an older app.
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

describe("the demo's copies of Rust tables", () => {
  const providers = rust("crates/writ-core/src/ai/providers.rs");

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

  it("offers the tools writ_core::tools offers", () => {
    const tools = rust("crates/writ-core/src/tools.rs");
    expect(MCP_TOOLS.read).toEqual(list(tools, "READ_TOOLS"));
    expect(MCP_TOOLS.write).toEqual(list(tools, "WRITE_TOOLS"));
  });
});
