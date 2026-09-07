import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// `App.tsx` is the one place commands are registered, and it registers them at
// mount with closures over stores no test can stand up. Reading the file is how
// the suite sees the registry's contents without running the app.

const APP_TSX = readFileSync(resolve(process.cwd(), "src/App.tsx"), "utf8");

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

/** Strips the quotes, and the escaping a TypeScript literal needs. */
export function literalText(raw: string): string {
  return raw.replace(/^["'`]|["'`]$/g, "").replace(/\\\\/g, "\\");
}

/** The elements of a single-line array literal, unquoted. */
function literalList(raw: string): string[] {
  const inner = raw.match(/^\[(.*)\]$/)?.[1];
  if (!inner) return [];
  return inner
    .split(",")
    .map((part) => literalText(part.trim()))
    .filter((part) => part.length > 0);
}

export interface Registration {
  label: string;
  keybinding: string;
  aliases: string[];
}

/** Every command `App.tsx` registers, by id, as written in the file. */
function registrations(): Map<string, Registration> {
  const found = new Map<string, Registration>();
  const marker = "registerCommand({";
  let at = APP_TSX.indexOf(marker);
  while (at !== -1) {
    const literal = objectLiteral(APP_TSX, at + marker.length - 1);
    const id = literalText(field(literal, "id"));
    if (id) {
      found.set(id, {
        label: literalText(field(literal, "label")),
        keybinding: literalText(field(literal, "keybinding")),
        aliases: literalList(field(literal, "keybindingAliases")),
      });
    }
    at = APP_TSX.indexOf(marker, at + marker.length);
  }
  return found;
}

export const APP_REGISTRATIONS = registrations();
