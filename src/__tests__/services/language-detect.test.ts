import { describe, it, expect } from "vitest";
import {
  detectLanguage,
  detectFromContent,
  extensionOf,
  namesPlainText,
  textMayNameLanguage,
  MAX_DETECT_BYTES,
} from "../../services/language-detect";

describe("detectLanguage", () => {
  describe("extension-based detection", () => {
    it.each([
      ["file.js", "javascript"],
      ["file.jsx", "javascript"],
      ["file.ts", "typescript"],
      ["file.tsx", "tsx"],
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
      ["file.zsh", "shell"],
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

  // ADR-041 §2: a .txt file gets the plain editor. A fence's info string names
  // the language of the fence, never of the file around it.
  describe("a fence never names the file's language", () => {
    it("keeps a .txt holding a python fence plain", () => {
      const content = "Groceries\n\n```python\nprint('hi')\n```\n";
      expect(detectLanguage(content, "list.txt")).toBeNull();
    });

    it("keeps a file whose extension the map does not know plain", () => {
      const content = "# Readme\n\n```typescript\nconst x = 1;\n```";
      expect(detectLanguage(content, "server.log")).toBeNull();
    });

    it("keeps a name with no extension plain when only a fence speaks", () => {
      expect(detectLanguage("intro\n```js\nlet a = 1\n```\n", "README")).toBeNull();
      expect(detectLanguage("```JavaScript\ncode\n```")).toBeNull();
    });

    it("leaves an unlabelled fence plain too", () => {
      expect(detectLanguage("```\nplain code\n```", "notes.txt")).toBeNull();
    });
  });

  describe("a name that spells a format decides", () => {
    it("keeps a .txt plain whatever its first line says", () => {
      expect(detectLanguage("#!/bin/bash\necho hi\n", "script.txt")).toBeNull();
      expect(detectLanguage("#!/usr/bin/python3\nprint(1)\n", "notes.TEXT")).toBeNull();
    });

    it("reads a shebang only when the name has no extension", () => {
      expect(detectLanguage("#!/bin/bash\necho hi\n", "deploy")).toBe("shell");
      expect(detectLanguage("#!/bin/bash\necho hi\n", "deploy.cgi")).toBeNull();
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

  describe("input cap (perf)", () => {
    // A JSON body whose closing brace sits past the detection cap.
    const hugeJsonBody = () => "{\n" + '  "k": "' + "v".repeat(MAX_DETECT_BYTES * 2) + '"\n}';

    it("detects from anchors inside the first 64 KiB even when the doc is huge", () => {
      const header = `use std::collections::HashMap;\n\nstruct Buffer { id: String }\n\nfn main() {\n    let buf = Buffer { id: String::new() };\n}`;
      const huge = header + "\n" + "x".repeat(MAX_DETECT_BYTES * 2);
      expect(detectFromContent(huge)).toBe("rust");
    });

    it("does not parse a multi-megabyte JSON body whole (truncated tail fails the structural check)", () => {
      // A JSON object whose closing brace sits past the 64 KiB cap: the cheap
      // first/last-char structural check sees a non-`}` tail and bails before
      // any JSON.parse, so a giant doc is never fully parsed for detection.
      expect(detectFromContent(hugeJsonBody())).not.toBe("json");
    });

    // Wall-clock budget for the same input. A loaded machine misses it for
    // reasons that have nothing to do with the code, so it runs only under
    // WRIT_PERF_GATE=1: scripts/perf-gate.sh drives it, on an otherwise idle
    // box (docs/perf-budgets.md).
    it.skipIf(!process.env.WRIT_PERF_GATE)("bails out of the huge JSON body in under 16ms", () => {
      const big = hugeJsonBody();
      const start = performance.now();
      const result = detectFromContent(big);
      const elapsedMs = performance.now() - start;
      expect(result).not.toBe("json");
      expect(elapsedMs).toBeLessThan(16);
    });

    it("only ever scores at most MAX_DETECT_BYTES of input", () => {
      // Anchors planted only beyond the cap must not be found: proves scoring
      // never reaches past the head slice.
      const tailOnly = "plain text preamble.\n".repeat(Math.ceil(MAX_DETECT_BYTES / 10)) +
        "\ninterface Late { id: string; }\ntype X = number;\n";
      expect(tailOnly.length).toBeGreaterThan(MAX_DETECT_BYTES);
      expect(detectFromContent(tailOnly)).toBeNull();
    });
  });
});

describe("extensionOf", () => {
  it("answers the last extension, lower case", () => {
    expect(extensionOf("Main.RS")).toBe("rs");
    expect(extensionOf("archive.tar.GZ")).toBe("gz");
  });

  it("reads only the last part of a path", () => {
    expect(extensionOf("/Users/me/notes.d/README")).toBeNull();
    expect(extensionOf("C:\\Users\\me\\build.d\\deploy")).toBeNull();
    expect(extensionOf("C:\\Users\\me\\deploy.yaml")).toBe("yaml");
  });

  it("gives a leading dot no extension", () => {
    expect(extensionOf(".bashrc")).toBeNull();
    expect(extensionOf(".bashrc.sh")).toBe("sh");
  });

  it("gives a trailing dot no extension", () => {
    expect(extensionOf("notes.")).toBeNull();
  });
});

describe("namesPlainText", () => {
  it("is true for .txt and .text in any case", () => {
    expect(namesPlainText("list.txt")).toBe(true);
    expect(namesPlainText("notes.TEXT")).toBe(true);
  });

  it("is false for any other name", () => {
    expect(namesPlainText("server.log")).toBe(false);
    expect(namesPlainText("txt")).toBe(false);
    expect(namesPlainText("main.rs")).toBe(false);
  });
});

describe("textMayNameLanguage", () => {
  it("lets the text speak only for a name with no extension", () => {
    expect(textMayNameLanguage("deploy")).toBe(true);
    expect(textMayNameLanguage(undefined)).toBe(true);
    expect(textMayNameLanguage("")).toBe(true);
    expect(textMayNameLanguage("list.txt")).toBe(false);
    expect(textMayNameLanguage("server.log")).toBe(false);
  });
});
