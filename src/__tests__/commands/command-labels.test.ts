import { describe, it, expect, afterEach } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { getAllCommands, unregisterCommand } from "../../commands/registry";
import { registerEditorCommands, EDITOR_COMMANDS } from "../../editor/editor-commands";
import { registerAiCommands, unregisterAiCommands } from "../../commands/ai";
import { REWRITE_ACTIONS } from "../../commands/rewrite-actions";

// A screen reader reads the label and nothing else, so a command with an empty
// one is registered but unreachable: it has a row in the palette and a line in
// the menu that announce nothing. Every registration is checked at its source,
// and the menu's own id list is checked against those sources.

const ROOT = process.cwd();
const SRC = resolve(ROOT, "src");
const APP_MENU = readFileSync(resolve(SRC, "components/TitleBar/AppMenu.tsx"), "utf8");

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "__tests__" || entry === "node_modules" || entry === "generated") continue;
      walk(full, files);
    } else if (entry.endsWith(".ts") || entry.endsWith(".tsx")) {
      files.push(full);
    }
  }
  return files;
}

interface Registration {
  file: string;
  id: string;
  label: string;
}

/** The object literal passed to a `registerCommand({ … })` call. */
function objectLiteral(text: string, open: number): string {
  let depth = 0;
  for (let i = open; i < text.length; i += 1) {
    if (text[i] === "{") depth += 1;
    else if (text[i] === "}") {
      depth -= 1;
      if (depth === 0) return text.slice(open + 1, i);
    }
  }
  throw new Error("unterminated registerCommand literal");
}

function field(literal: string, name: string): string {
  const match = literal.match(new RegExp(`(^|[\\s{,])${name}:\\s*([^\\n]+?),?\\s*$`, "m"));
  return match ? match[2].trim() : "";
}

function registrations(): Registration[] {
  const found: Registration[] = [];
  for (const file of walk(SRC)) {
    const text = readFileSync(file, "utf8");
    const marker = "registerCommand({";
    let at = text.indexOf(marker);
    while (at !== -1) {
      const literal = objectLiteral(text, at + marker.length - 1);
      found.push({
        file: relative(ROOT, file),
        id: field(literal, "id"),
        label: field(literal, "label"),
      });
      at = text.indexOf(marker, at + marker.length);
    }
  }
  return found;
}

const REGISTRATIONS = registrations();

/** A label written in the file, rather than read from a table at runtime. */
function literalText(label: string): string | null {
  const quoted = label.match(/^"([^"]*)"$/) ?? label.match(/^'([^']*)'$/);
  if (quoted) return quoted[1];
  const template = label.match(/^`(.*)`$/);
  if (template) return template[1];
  return null;
}

afterEach(() => {
  unregisterAiCommands();
  for (const spec of EDITOR_COMMANDS) unregisterCommand(spec.id);
});

describe("command labels", () => {
  it("finds every registration in the app, not a handful", () => {
    expect(REGISTRATIONS.length).toBeGreaterThan(30);
  });

  it("gives every registered command a label", () => {
    const unlabelled = REGISTRATIONS.filter((r) => r.label === "").map(
      (r) => `${r.file}:${r.id || "?"}`,
    );
    expect(unlabelled, `no label: ${unlabelled.join(", ")}`).toEqual([]);
  });

  it("never registers an empty label", () => {
    const empty = REGISTRATIONS.filter((r) => {
      const text = literalText(r.label);
      return text !== null && text.trim() === "";
    }).map((r) => `${r.file}:${r.id || "?"}`);
    expect(empty, `empty label: ${empty.join(", ")}`).toEqual([]);
  });

  it("labels every command in the tables the registrations read", () => {
    for (const spec of EDITOR_COMMANDS) expect(spec.label.trim(), spec.id).not.toBe("");
    for (const action of REWRITE_ACTIONS) expect(action.label.trim(), action.id).not.toBe("");
  });

  it("labels every command the palette would list", () => {
    registerEditorCommands(() => null);
    registerAiCommands();
    const commands = getAllCommands();
    expect(commands.length).toBeGreaterThan(0);
    for (const command of commands) expect(command.label.trim(), command.id).not.toBe("");
  });

  it("names only commands that are registered somewhere in the menu", () => {
    const block = APP_MENU.match(/const MENU_COMMAND_IDS = \[([\s\S]*?)\] as const;/);
    expect(block, "AppMenu declares MENU_COMMAND_IDS").toBeTruthy();
    const menuIds = Array.from(block![1].matchAll(/"([^"]+)"/g)).map((m) => m[1]);
    expect(menuIds.length).toBeGreaterThan(0);
    const registeredIds = new Set(REGISTRATIONS.map((r) => literalText(r.id)).filter(Boolean));
    const missing = menuIds.filter((id) => !registeredIds.has(id));
    expect(missing, `menu ids with no registration: ${missing.join(", ")}`).toEqual([]);
  });
});
