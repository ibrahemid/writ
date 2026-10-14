import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { EditorView } from "@codemirror/view";
import { language as languageFacet } from "@codemirror/language";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";
import { getCommand } from "../../../commands/registry";

// What language a file is shown in: the name decides, a .txt stays plain text
// whatever it holds or gains, every detected language is highlighted, and the
// language the person chose for a file wins over all of it.

const bufferContent = new Map<string, string>();
const openLanguagePicker = vi.hoisted(() => vi.fn());

vi.mock("../LanguagePicker", () => ({ openLanguagePicker }));

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => {}),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

const PARSE_WAIT = { timeout: 25_000, interval: 20 };

function fileBuffer(id: string, name: string, language: string | null = null): BufferDocument {
  return {
    id,
    title: name,
    filename: `${id}.txt`,
    status: "active",
    language,
    source_path: `/files/${name}`,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
  };
}

async function flushMicrotasks(n = 10) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

let detected: (() => string | null) | undefined;

function Probe() {
  detected = useWindow().editor.language;
  return null;
}

async function mount(initial: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [buf, setBuf] = createSignal(initial);
  const { container } = render(() => (
    <WindowProvider windowId={9901}>
      <Probe />
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  const dom = container.querySelector<HTMLElement>(".cm-editor");
  if (!dom) throw new Error("the editor did not mount");
  const view = EditorView.findFromDOM(dom)!;
  return { container, view, setBuf };
}

function grammarOf(view: EditorView): string | null {
  return view.state.facet(languageFacet)?.name ?? null;
}

function hasGutter(container: HTMLElement): boolean {
  return container.querySelector(".cm-gutters") !== null;
}

describe("EditorInstance: the language a file is shown in", { timeout: 30_000 }, () => {
  beforeEach(() => {
    bufferContent.clear();
    detected = undefined;
  });

  afterEach(() => {
    cleanup();
  });

  it("opens a .txt holding a python fence as plain text", async () => {
    bufferContent.set("L1", "Groceries\n\n```python\nprint('hi')\n```\n");
    const { container, view } = await mount(fileBuffer("L1", "list.txt"));

    expect(detected!()).toBeNull();
    expect(grammarOf(view)).toBeNull();
    expect(hasGutter(container)).toBe(false);
  });

  it("opens a lowercase .text as plain text", async () => {
    bufferContent.set("L9", "Agenda\n\n- budget\n- hiring\n");
    const { container, view } = await mount(fileBuffer("L9", "meeting.text"));

    expect(detected!()).toBeNull();
    expect(grammarOf(view)).toBeNull();
    expect(hasGutter(container)).toBe(false);
  });

  it("keeps a .txt plain text when Python is typed into it", async () => {
    bufferContent.set("L2", "notes\n");
    const { container, view } = await mount(fileBuffer("L2", "notes.txt"));

    view.dispatch({
      changes: {
        from: view.state.doc.length,
        insert: "from os import path\n\ndef main():\n    print(path.sep)\n\nmain()\n",
      },
    });
    await flushMicrotasks();

    expect(detected!()).toBeNull();
    expect(grammarOf(view)).toBeNull();
    expect(hasGutter(container)).toBe(false);
  });

  it("highlights deploy.yaml", async () => {
    bufferContent.set("L3", "services:\n  web:\n    image: nginx\n");
    const { view } = await mount(fileBuffer("L3", "deploy.yaml"));

    expect(detected!()).toBe("yaml");
    await vi.waitFor(() => expect(grammarOf(view)).toBe("yaml"), PARSE_WAIT);
  });

  it("highlights a shell script", async () => {
    bufferContent.set("L4", "#!/bin/sh\necho hi\n");
    const { view } = await mount(fileBuffer("L4", "build.sh"));

    expect(detected!()).toBe("shell");
    await vi.waitFor(() => expect(grammarOf(view)).toBe("shell"), PARSE_WAIT);
  });

  it("shows a file in the language chosen for it", async () => {
    bufferContent.set("L5", "print('hi')\n");
    const { container, view } = await mount(fileBuffer("L5", "snippet.txt", "python"));

    expect(detected!()).toBe("python");
    expect(hasGutter(container)).toBe(true);
    await vi.waitFor(() => expect(grammarOf(view)).toBe("python"), PARSE_WAIT);
  });

  it("shows a file chosen as plain text as plain text", async () => {
    bufferContent.set("L6", "fn main() {}\n");
    const { container, view } = await mount(fileBuffer("L6", "main.rs", "plaintext"));

    expect(detected!()).toBeNull();
    expect(grammarOf(view)).toBeNull();
    expect(hasGutter(container)).toBe(false);
  });

  it("offers Change language, which opens the language list", async () => {
    bufferContent.set("L8", "text\n");
    await mount(fileBuffer("L8", "notes.txt"));

    const command = getCommand("editor.changeLanguage");
    expect(command?.label).toBe("Change language…");
    command!.execute();
    expect(openLanguagePicker).toHaveBeenCalledTimes(1);

    cleanup();
    expect(getCommand("editor.changeLanguage")).toBeUndefined();
  });

  it("follows a new choice while the file is open", async () => {
    bufferContent.set("L7", "key = 1\n");
    const { container, view, setBuf } = await mount(fileBuffer("L7", "settings.conf"));
    expect(detected!()).toBeNull();

    setBuf(fileBuffer("L7", "settings.conf", "toml"));
    await flushMicrotasks();

    expect(detected!()).toBe("toml");
    expect(hasGutter(container)).toBe(true);
    await vi.waitFor(() => expect(grammarOf(view)).toBe("toml"), PARSE_WAIT);

    setBuf(fileBuffer("L7", "settings.conf", null));
    await flushMicrotasks();

    expect(detected!()).toBeNull();
    expect(grammarOf(view)).toBeNull();
    expect(hasGutter(container)).toBe(false);
  });
});
