import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { undo } from "@codemirror/commands";
import { hashDocument } from "../../../lib/doc-hash";
import { handleExternalEdit, type ExternalEditDeps } from "../../../services/external-edit";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";
import type { WindowState } from "../../../stores/window/createWindowState";

// The file, as the operating system holds it. `readBufferContent` is the Rust
// command that reads it, so this map is what "on disk" means here.
const fileOnDisk = new Map<string, string>();

const readBufferContent = vi.fn(async (id: string) => fileOnDisk.get(id) ?? "");

vi.mock("../../../services/tauri", () => ({
  readBufferContent: (id: string) => readBufferContent(id),
  saveBufferContent: vi.fn(async () => null),
  // The digest comes from the same map the read does, so a note's record
  // describes the file the test is actually changing.
  noteDiskState: vi.fn(async (id: string) => {
    const text = fileOnDisk.get(id);
    if (text === undefined) return { state: "no_file" };
    const { hashDocument } = await import("../../../lib/doc-hash");
    return {
      state: "described",
      disk: { hash: await hashDocument(text), size: text.length, mtime_ms: null },
    };
  }),
}));

// The registry's read is a straight pass-through to the Rust command. Its
// `load` is deliberately absent: a reload path that called it would recreate
// the always-mounted preview iframe and hard-freeze the macOS webview
// (PR #127), so a call to it here fails the test rather than freezing it.
vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: (id: string) => readBufferContent(id),
  },
}));

function mockBuffer(id: string): BufferDocument {
  return {
    id,
    title: id,
    filename: `${id}.md`,
    status: "active",
    language: null,
    source_path: `/somewhere/else/${id}.md`,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

async function flushMicrotasks(n = 10) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

/** The same mount, with the tab in front under the test's control. */
async function mountSwitchable(first: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [buffer, setBuffer] = createSignal(first);
  let win!: WindowState;
  const Probe = () => {
    win = useWindow();
    return null;
  };
  const result = render(() => (
    <WindowProvider windowId={9402}>
      <Probe />
      <EditorInstance buffer={buffer()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return { ...result, win, switchTo: setBuffer };
}

async function mount(buffer: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  let win!: WindowState;
  const Probe = () => {
    win = useWindow();
    return null;
  };
  const result = render(() => (
    <WindowProvider windowId={9401}>
      <Probe />
      <EditorInstance buffer={buffer} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return { ...result, win };
}

describe("EditorInstance: reloading after a change outside Writ", () => {
  beforeEach(() => {
    fileOnDisk.clear();
    readBufferContent.mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it("shows what the file holds now, not what Writ was holding", async () => {
    // The whole point of the watcher: another program rewrote the file, and
    // the editor has to end up showing those bytes. Asserting the text in the
    // view rather than a call count, because a reload that read a copy Writ
    // was keeping would make the same calls and show the wrong thing.
    fileOnDisk.set("note", "as Writ opened it\n");
    const { container, win } = await mount(mockBuffer("note"));
    expect(container.textContent).toContain("as Writ opened it");

    fileOnDisk.set("note", "rewritten by another program\n");
    win.editor.requestExternalReload("note");

    await waitFor(() =>
      expect(container.textContent).toContain("rewritten by another program"),
    );
    expect(container.textContent).not.toContain("as Writ opened it");
  });

  it("goes back to the file for every change, not once", async () => {
    fileOnDisk.set("note", "first\n");
    const { container, win } = await mount(mockBuffer("note"));

    fileOnDisk.set("note", "second\n");
    win.editor.requestExternalReload("note");
    await waitFor(() => expect(container.textContent).toContain("second"));

    fileOnDisk.set("note", "third\n");
    win.editor.requestExternalReload("note");
    await waitFor(() => expect(container.textContent).toContain("third"));
  });
  it("leaves the reader on their line and where they had scrolled to", async () => {
    // A change five hundred lines into a file must not move the person
    // reading line two hundred of it. Replacing the whole document would put
    // them at the end of the insert, which is the bottom of the file.
    const lines = Array.from({ length: 500 }, (_, i) => `line ${i + 1}`);
    fileOnDisk.set("note", `${lines.join("\n")}\n`);
    const { win } = await mount(mockBuffer("note"));
    const view = win.editor.getView()!;

    const line200 = view.state.doc.line(200);
    view.dispatch({ selection: { anchor: line200.from + 5 } });
    view.scrollDOM.scrollTop = 1234;
    // Assert the environment actually kept it, so the check below cannot
    // pass by comparing zero with zero.
    expect(view.scrollDOM.scrollTop).toBe(1234);

    const rewritten = [...lines];
    rewritten[249] = "line 250, rewritten by another program";
    fileOnDisk.set("note", `${rewritten.join("\n")}\n`);
    win.editor.requestExternalReload("note");

    await waitFor(() =>
      expect(view.state.doc.toString()).toContain("rewritten by another program"),
    );
    const head = view.state.selection.main.head;
    expect(view.state.doc.lineAt(head).number).toBe(200);
    expect(view.state.doc.lineAt(head).text).toBe("line 200");
    expect(view.scrollDOM.scrollTop).toBe(1234);
  });

  it("puts the cursor on the nearest line a shorter file has", async () => {
    fileOnDisk.set("note", "one\ntwo\nthree\nfour\n");
    const { win } = await mount(mockBuffer("note"));
    const view = win.editor.getView()!;
    view.dispatch({ selection: { anchor: view.state.doc.line(4).from } });

    fileOnDisk.set("note", "one\n");
    win.editor.requestExternalReload("note");

    await waitFor(() => expect(view.state.doc.toString()).toBe("one\n"));
    const head = view.state.selection.main.head;
    expect(view.state.doc.lineAt(head).number).toBe(view.state.doc.lines);
  });

  it("gives the whole reload back for one undo", async () => {
    // One transaction, so one Cmd+Z. A person who did not want the file's
    // version gets their text back with the keystroke they already know.
    fileOnDisk.set("note", "as Writ opened it\n");
    const { win } = await mount(mockBuffer("note"));
    const view = win.editor.getView()!;

    fileOnDisk.set("note", "rewritten by another program\n");
    win.editor.requestExternalReload("note");
    await waitFor(() =>
      expect(view.state.doc.toString()).toBe("rewritten by another program\n"),
    );

    undo(view);

    expect(view.state.doc.toString()).toBe("as Writ opened it\n");
  });

  it("keeps a tab in the background up to date with its file", async () => {
    // The reload used to be dropped for any tab that was not in front. The
    // text came back on the switch, but the record did not move, so the note
    // read dirty against a file it matched and the next change to it asked a
    // question with no reason to be asked.
    fileOnDisk.set("front", "the tab in front\n");
    fileOnDisk.set("behind", "as Writ opened it\n");
    const { win, switchTo } = await mountSwitchable(mockBuffer("front"));
    win.editor.noteOpened("behind", "as Writ opened it\n");
    await waitFor(() => expect(win.editor.isDirty("behind")).toBe(false));

    fileOnDisk.set("behind", "rewritten while it was behind\n");
    win.editor.requestExternalReload("behind");

    const digest = await hashDocument("rewritten while it was behind\n");
    await waitFor(() => expect(win.editor.lastKnownDiskHash("behind")).toBe(digest));
    expect(win.editor.isDirty("behind")).toBe(false);
    expect(win.editor.isUpdatedFromDisk("behind")).toBe(true);
    // The tab in front was not touched by a reload aimed at another note.
    expect(win.editor.getView()!.state.doc.toString()).toBe("the tab in front\n");

    switchTo(mockBuffer("behind"));
    await waitFor(() =>
      expect(win.editor.getView()!.state.doc.toString()).toBe(
        "rewritten while it was behind\n",
      ),
    );
  });

  it("replaces nothing under unsaved text, whatever the hashing is doing", async () => {
    // The dirty predicate is asynchronous and the watcher is not. The race
    // this closes is a change arriving in the window between a keystroke and
    // its digest, which is exactly when a person is typing.
    fileOnDisk.set("note", "as Writ opened it\n");
    const { win } = await mount(mockBuffer("note"));
    const view = win.editor.getView()!;
    await waitFor(() => expect(win.editor.isDirty("note")).toBe(false));

    view.dispatch({
      changes: { from: view.state.doc.length, insert: "typed and not saved\n" },
    });
    const typed = view.state.doc.toString();

    const asked: string[] = [];
    const deps: ExternalEditDeps = {
      findBuffer: () => ({ id: "note", title: "note.md" }),
      hasUnsaved: (id) => win.editor.isDirty(id),
      isRemovedOnDisk: (id) => win.editor.isRemovedOnDisk(id),
      reload: (id) => win.editor.requestExternalReload(id),
      markChanged: (id) => asked.push(id),
      followMove: () => expect.unreachable("a modification is not a move"),
      markRemoved: () => expect.unreachable("a modification is not a removal"),
    };

    // Before the digest of the keystroke has landed.
    fileOnDisk.set("note", "rewritten by another program\n");
    await handleExternalEdit({ bufferId: "note", change: "modified" }, deps);
    expect(view.state.doc.toString()).toBe(typed);
    expect(asked).toEqual(["note"]);

    // And after it has.
    const digest = await hashDocument(typed);
    await waitFor(() => expect(win.editor.docHash("note")).toBe(digest));
    await handleExternalEdit({ bufferId: "note", change: "modified" }, deps);
    expect(view.state.doc.toString()).toBe(typed);
    expect(asked).toEqual(["note", "note"]);
  });
});
