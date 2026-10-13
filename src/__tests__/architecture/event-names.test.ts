import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const EMITTER = readFileSync(resolve(process.cwd(), "src-tauri/src/events/emitter.rs"), "utf8");
const EVENTS_TS = readFileSync(resolve(process.cwd(), "src/services/events.ts"), "utf8");

function rustKinds(): Map<string, string> {
  const kinds = new Map<string, string>();
  const pattern = /#\[serde\(rename = "([^"]+)"\)\]\s*(?:#\[[^\]]*\]\s*)*([A-Z][A-Za-z]+)/g;
  for (const match of EMITTER.matchAll(pattern)) kinds.set(match[2]!, match[1]!);
  return kinds;
}

function rustChannels(): Map<string, string> {
  const channels = new Map<string, string>();
  const pattern = /WritFrontendEvent::([A-Z][A-Za-z]+)\s*(?:\{\s*\.\.\s*\}|\(\.\.\))\s*=>\s*"([^"]+)"/g;
  for (const match of EMITTER.matchAll(pattern)) channels.set(match[1]!, match[2]!);
  return channels;
}

function frontendChannels(): Map<string, string> {
  const start = EVENTS_TS.indexOf("const EVENT_MAP");
  expect(start, "EVENT_MAP is declared in src/services/events.ts").toBeGreaterThan(-1);
  const body = EVENTS_TS.slice(start, EVENTS_TS.indexOf("};", start));
  return new Map([...body.matchAll(/"([^"]+)":\s*"([^"]+)"/g)].map((match) => [match[1]!, match[2]!]));
}

describe("every event the app emits is heard on the channel it is sent on", () => {
  const kinds = rustKinds();
  const channels = rustChannels();
  const frontend = frontendChannels();

  it("reads both sides", () => {
    expect(kinds.size).toBeGreaterThan(0);
    expect(channels.size).toBe(kinds.size);
    expect(kinds.get("McpPending")).toBe("mcp:pending");
  });

  it("maps each kind to the channel the emitter names for it", () => {
    for (const [variant, kind] of kinds) {
      expect(frontend.get(kind), `${variant} (${kind})`).toBe(channels.get(variant));
    }
  });

  it("listens for no kind the emitter never sends", () => {
    const sent = new Set(kinds.values());
    for (const kind of frontend.keys()) expect(sent.has(kind), kind).toBe(true);
  });
});
