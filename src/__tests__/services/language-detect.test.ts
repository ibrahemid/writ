import { describe, it, expect } from "vitest";
import { detectLanguage, detectFromContent } from "../../services/language-detect";

describe("detectLanguage", () => {
  describe("extension-based detection", () => {
    it.each([
      ["file.js", "javascript"],
      ["file.jsx", "javascript"],
      ["file.ts", "typescript"],
      ["file.tsx", "typescript"],
      ["file.py", "python"],
      ["file.rb", "ruby"],
      ["file.rs", "rust"],
      ["file.go", "go"],
      ["file.java", "java"],
      ["file.php", "php"],
      ["file.html", "html"],
      ["file.css", "css"],
      ["file.json", "json"],
      ["file.md", "markdown"],
      ["file.yaml", "yaml"],
      ["file.yml", "yaml"],
      ["file.sql", "sql"],
      ["file.sh", "shell"],
      ["file.bash", "shell"],
      ["file.toml", "toml"],
      ["file.xml", "xml"],
    ])("detects %s as %s", (filename, expected) => {
      expect(detectLanguage("", filename)).toBe(expected);
    });

    it("is case-insensitive for extensions", () => {
      expect(detectLanguage("", "File.RS")).toBe("rust");
      expect(detectLanguage("", "MODULE.TS")).toBe("typescript");
    });

    it("handles dotfiles with known extension", () => {
      expect(detectLanguage("", ".bashrc.sh")).toBe("shell");
    });

    it("takes priority over shebang", () => {
      expect(detectLanguage("#!/usr/bin/python3\nprint('hi')", "script.rs")).toBe("rust");
    });
  });

  describe("shebang-based detection", () => {
    it.each([
      ["#!/usr/bin/python", "python"],
      ["#!/usr/bin/python3", "python"],
      ["#!/usr/bin/node", "javascript"],
      ["#!/bin/bash", "shell"],
      ["#!/bin/sh", "shell"],
      ["#!/usr/bin/zsh", "shell"],
      ["#!/usr/bin/ruby", "ruby"],
      ["#!/usr/bin/perl", "perl"],
      ["#!/usr/bin/php", "php"],
    ])("detects shebang %s as %s", (shebang, expected) => {
      expect(detectLanguage(shebang + "\ncode here")).toBe(expected);
    });

    it("does not resolve env-style shebangs", () => {
      expect(detectLanguage("#!/usr/bin/env node\ncode")).toBeNull();
    });

    it("ignores unknown shebangs", () => {
      expect(detectLanguage("#!/usr/bin/unknown\ncode")).toBeNull();
    });
  });

  describe("code fence detection", () => {
    it("detects language from first code fence", () => {
      const content = "# Readme\n\n```typescript\nconst x = 1;\n```";
      expect(detectLanguage(content)).toBe("typescript");
    });

    it("lowercases the fence language", () => {
      const content = "```JavaScript\ncode\n```";
      expect(detectLanguage(content)).toBe("javascript");
    });

    it("ignores fences without a language tag", () => {
      const content = "```\nplain code\n```";
      expect(detectLanguage(content)).toBeNull();
    });
  });

  describe("fallback", () => {
    it("returns null for empty content and no filename", () => {
      expect(detectLanguage("")).toBeNull();
    });

    it("returns null for plain text content", () => {
      expect(detectLanguage("just some plain text here")).toBeNull();
    });

    it("returns null for unknown file extension", () => {
      expect(detectLanguage("", "file.xyz")).toBeNull();
    });

    it("returns null when filename has no extension", () => {
      expect(detectLanguage("", "Makefile")).toBeNull();
    });
  });
});

describe("detectFromContent", () => {
  it("detects rust from fn + use + struct", () => {
    const text = `use std::collections::HashMap;\n\nstruct Buffer { id: String, content: String }\n\nfn main() {\n    let mut buf = Buffer { id: String::new(), content: String::new() };\n    println!("{}", buf.id);\n}`;
    expect(detectFromContent(text)).toBe("rust");
  });

  it("detects typescript from interface + type annotations", () => {
    const text = `import { createSignal } from "solid-js";\n\ninterface Buffer {\n  id: string;\n  content: string;\n}\n\nexport function makeBuffer(id: string): Buffer {\n  return { id, content: "" };\n}`;
    expect(detectFromContent(text)).toBe("typescript");
  });

  it("detects python from def + from import + class", () => {
    const text = `from typing import Optional\n\nclass Buffer:\n    def __init__(self, id: str):\n        self.id = id\n        self.content = ""\n\n    def save(self) -> None:\n        print(self.id)`;
    expect(detectFromContent(text)).toBe("python");
  });

  it("detects json from a parsable object", () => {
    const text = `{\n  "name": "writ",\n  "version": "0.1.0",\n  "private": true,\n  "dependencies": {\n    "solid-js": "^1.9.0"\n  }\n}`;
    expect(detectFromContent(text)).toBe("json");
  });

  it("detects markdown from headings + lists", () => {
    const text = `# Writ\n\n## Overview\n\nA lightweight editor.\n\n- Always-ready scratchpad\n- Autosave\n- Full-text search\n\nSee [docs](docs/ARCHITECTURE.md) for details.`;
    expect(detectFromContent(text)).toBe("markdown");
  });

  it("returns null for whitespace-only input", () => {
    expect(detectFromContent("   \n\n  \t")).toBeNull();
  });

  it("returns null for content shorter than threshold", () => {
    expect(detectFromContent("fn x()")).toBeNull();
  });

  it("returns null for ambiguous low-signal text", () => {
    const text = "this is a plain paragraph of english words with no syntactic anchors at all.";
    expect(detectFromContent(text)).toBeNull();
  });

  it("returns null for almost-json (invalid syntax)", () => {
    const text = `{\n  name: writ,\n  version: 0.1.0\n}`;
    expect(detectFromContent(text)).toBeNull();
  });

  it("detects bash from shebang content", () => {
    const text = `#!/bin/bash\nset -euo pipefail\nfor file in *.txt; do\n  echo "$file"\ndone`;
    expect(detectFromContent(text)).toBe("shell");
  });

  it("detects html from doctype", () => {
    const text = `<!DOCTYPE html>\n<html>\n<head><title>x</title></head>\n<body><div class="x">hi</div></body>\n</html>`;
    expect(detectFromContent(text)).toBe("html");
  });
});
