import { describe, it, expect } from "vitest";
import { detectLanguage } from "../../services/language-detect";

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
