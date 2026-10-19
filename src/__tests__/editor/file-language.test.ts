import { describe, it, expect } from "vitest";
import { Text } from "@codemirror/state";
import { contentMayNameLanguage, languageHeadOf, languageOfFile } from "../../editor/file-language";
import { MAX_DETECT_BYTES } from "../../services/language-detect";

const onDisk = (name: string, content = "", chosen: string | null = null) => ({
  chosen,
  name,
  hasFile: true,
  content,
});

const untitled = (name: string, content = "", chosen: string | null = null) => ({
  chosen,
  name,
  hasFile: false,
  content,
});

describe("languageOfFile", () => {
  it("lets the name decide a file on disk", () => {
    expect(languageOfFile(onDisk("deploy.yaml"))).toBe("yaml");
    expect(languageOfFile(onDisk("page.tsx"))).toBe("tsx");
    expect(languageOfFile(onDisk("main.c"))).toBe("c");
    expect(languageOfFile(onDisk("Dockerfile"))).toBe("dockerfile");
  });

  it("keeps a .txt plain whatever it holds", () => {
    expect(languageOfFile(onDisk("list.txt", "```python\nprint('hi')\n```\n"))).toBeNull();
    expect(languageOfFile(onDisk("script.txt", "#!/bin/bash\necho hi\n"))).toBeNull();
  });

  it("answers a .text exactly as it answers a .txt", () => {
    const contents = ["", "Agenda\n", "```python\nprint('hi')\n```\n", "#!/bin/bash\necho hi\n"];
    for (const content of contents) {
      for (const make of [onDisk, untitled]) {
        expect(languageOfFile(make("meeting.text", content))).toBe(
          languageOfFile(make("meeting.txt", content)),
        );
      }
    }
    expect(languageOfFile(onDisk("meeting.text"))).toBeNull();
  });

  it("reads a shebang only for a name with no extension", () => {
    expect(languageOfFile(onDisk("deploy", "#!/bin/bash\necho hi\n"))).toBe("shell");
    expect(languageOfFile(onDisk("README", "intro\n```js\nlet a = 1\n```\n"))).toBeNull();
  });

  it("shows a file in the language chosen for it", () => {
    expect(languageOfFile(onDisk("snippet.txt", "print('hi')\n", "python"))).toBe("python");
    expect(languageOfFile(untitled("Untitled", "", "rust"))).toBe("rust");
  });

  it("shows a file chosen as plain text as plain text", () => {
    expect(languageOfFile(onDisk("main.rs", "fn main() {}\n", "plaintext"))).toBeNull();
    expect(languageOfFile(untitled("Untitled", "", "plaintext"))).toBeNull();
  });

  it("treats a choice the editor does not know as no choice", () => {
    expect(languageOfFile(onDisk("deploy.yaml", "", "no-such-language"))).toBe("yaml");
  });

  it("keeps an untitled file on a Markdown surface", () => {
    expect(languageOfFile(untitled("Untitled", "def main():\n    pass\n"))).toBe("markdown");
    // The `<id>.txt` an untitled row is filed under names nothing.
    expect(languageOfFile(untitled("demo-1.txt", "# Heading\n"))).toBe("markdown");
  });

  it("lets a name typed for an untitled file decide", () => {
    expect(languageOfFile(untitled("main.rs"))).toBe("rust");
  });

  it("reads a shebang at the top of an untitled file", () => {
    expect(languageOfFile(untitled("Untitled", "#!/usr/bin/python3\nprint(1)\n"))).toBe("python");
    expect(languageOfFile(untitled("Untitled", "#!/usr/bin/unknown\n"))).toBeNull();
  });
});

describe("contentMayNameLanguage", () => {
  it("is true only when nobody chose a language and the name says nothing", () => {
    expect(contentMayNameLanguage({ chosen: null, name: "deploy" })).toBe(true);
    expect(contentMayNameLanguage({ chosen: null, name: "notes.txt" })).toBe(false);
    expect(contentMayNameLanguage({ chosen: null, name: "server.log" })).toBe(false);
    expect(contentMayNameLanguage({ chosen: "python", name: "deploy" })).toBe(false);
    expect(contentMayNameLanguage({ chosen: "plaintext", name: "deploy" })).toBe(false);
  });
});

describe("languageHeadOf", () => {
  it("names the language its document's first line names", () => {
    const doc = Text.of(["#!/bin/bash", "echo hi", ""]);
    expect(languageOfFile(onDisk("deploy", languageHeadOf(doc)))).toBe("shell");
  });

  it("reads no further than detection does on a document of one long line", () => {
    const doc = Text.of(["#!/bin/sh " + "x".repeat(MAX_DETECT_BYTES * 4)]);
    expect(languageHeadOf(doc).length).toBe(MAX_DETECT_BYTES);
  });
});
