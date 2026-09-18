import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// The composer is what a person aims at a model: the words, the notes the
// reply may read, and the key that gets them there. Everything here is about
// that field and the row of chips above it.

const mocks = vi.hoisted(() => ({
  draft: "",
  attachments: [] as unknown[],
  status: "idle",
  editing: null as number | null,
  /** A reactive read of `editing`, for the case that flips it after mount. */
  editingRead: null as null | (() => number | null),
  setDraft: vi.fn(),
  detach: vi.fn(),
  attachByPath: vi.fn(),
  addOpenNote: vi.fn(),
  cancelEdit: vi.fn(),
  stop: vi.fn(),
  send: vi.fn(),
  candidates: vi.fn(),
}));

vi.mock("../../stores/global/chat", async () => {
  const actual = await vi.importActual<typeof import("../../stores/global/chat")>(
    "../../stores/global/chat",
  );
  return {
    ...actual,
    chatStore: {
      draft: () => mocks.draft,
      setDraft: mocks.setDraft,
      attachments: () => mocks.attachments,
      status: () => mocks.status,
      editing: () => (mocks.editingRead ? mocks.editingRead() : mocks.editing),
      detach: mocks.detach,
      attachByPath: mocks.attachByPath,
      addOpenNote: mocks.addOpenNote,
      cancelEdit: mocks.cancelEdit,
      stop: mocks.stop,
    },
  };
});

vi.mock("../../stores/global/link", () => ({
  linkStore: { noteNameCandidates: mocks.candidates },
}));

vi.mock("../../commands/chat", () => ({
  byteLabel: (bytes: number) => `${bytes} bytes`,
  sendChatMessage: mocks.send,
}));

vi.mock("../../components/Chat/ChatConnectionControl", () => ({
  default: () => <button type="button">Connection</button>,
  openConnectionControl: vi.fn(),
}));

import { createSignal } from "solid-js";
import ChatComposer from "../../components/Chat/ChatComposer";

function chip(path: string, extra: Record<string, unknown> = {}) {
  return { path, name: path.split("/").pop(), bytes: 10, key: path, ...extra };
}

/** What the chip says under a settled pointer. */
function tipOver(container: HTMLElement): string {
  fireEvent.pointerEnter(container.querySelector(".chat-chip .writ-tooltip-anchor") as Element);
  vi.advanceTimersByTime(500);
  return document.querySelector('[role="tooltip"]')?.textContent ?? "";
}

/** The field, which every case reaches for. */
function field(container: HTMLElement): HTMLTextAreaElement {
  return container.querySelector(".chat-composer-input") as HTMLTextAreaElement;
}

function mount(props: Record<string, unknown> = {}) {
  const onClose = vi.fn();
  const result = render(() => (
    <ChatComposer
      openNote={() => (props.openNote as "ready" | "unsaved" | "none") ?? "ready"}
      onClose={onClose}
    />
  ));
  return { ...result, onClose };
}

beforeEach(() => {
  mocks.draft = "";
  mocks.attachments = [];
  mocks.status = "idle";
  mocks.editing = null;
  mocks.editingRead = null;
  mocks.setDraft.mockReset();
  mocks.detach.mockReset();
  mocks.attachByPath.mockReset();
  mocks.addOpenNote.mockReset().mockResolvedValue({ ok: true, path: "Launch.md" });
  mocks.cancelEdit.mockReset();
  mocks.stop.mockReset();
  mocks.send.mockReset();
  mocks.candidates.mockReset().mockResolvedValue([]);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("the composer field", () => {
  it("grows with the draft and stops at its maximum", () => {
    const { container } = mount();
    const el = field(container);
    Object.defineProperty(el, "scrollHeight", { value: 220, configurable: true });

    el.value = "a\nb\nc\nd\ne";
    fireEvent.input(el);

    expect(el.style.height).toBe("220px");
    // The ceiling is the stylesheet's, so the field scrolls rather than
    // pushing the transcript off the top of the column.
    expect(el.style.maxHeight).toBe("");
    expect(el.getAttribute("rows")).toBe("2");
  });

  it("fits a draft it did not type", () => {
    // A send clears the field and an edit fills it; neither goes through an
    // input event, and the field still has to end up the right height.
    const held = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollHeight");
    Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
      value: 180,
      configurable: true,
    });
    mocks.draft = "a\nb\nc\nd\ne\nf";

    const { container } = mount();

    expect(field(container).style.height).toBe("180px");
    if (held) Object.defineProperty(HTMLElement.prototype, "scrollHeight", held);
  });

  it("takes focus when an edit begins, with the caret after the turn", () => {
    // Edit is pressed on a turn, not in the field. Whoever pressed it is
    // about to type, so the field is where the next key goes and the caret
    // sits after the words it was filled with.
    const [editing, setEditing] = createSignal<number | null>(null);
    mocks.editingRead = editing;
    mocks.draft = "what the note argues";
    const { container } = mount();
    const el = field(container);
    expect(document.activeElement).not.toBe(el);

    setEditing(1);

    expect(document.activeElement).toBe(el);
    expect(el.selectionStart).toBe(el.value.length);
    expect(el.selectionEnd).toBe(el.value.length);
  });

  it("cancels an edit, then stops a reply, then closes the pane", () => {
    mocks.editing = 2;
    const editing = mount();
    fireEvent.keyDown(field(editing.container), { key: "Escape" });
    expect(mocks.cancelEdit).toHaveBeenCalledTimes(1);
    expect(editing.onClose).not.toHaveBeenCalled();
    cleanup();

    mocks.editing = null;
    mocks.status = "streaming";
    const live = mount();
    fireEvent.keyDown(field(live.container), { key: "Escape" });
    expect(mocks.stop).toHaveBeenCalledTimes(1);
    expect(live.onClose).not.toHaveBeenCalled();
    cleanup();

    mocks.status = "idle";
    const quiet = mount();
    fireEvent.keyDown(field(quiet.container), { key: "Escape" });
    expect(quiet.onClose).toHaveBeenCalledTimes(1);
  });
});

describe("the chip row", () => {
  it("attaches the note in front, and says why it cannot", async () => {
    const ready = mount();
    const add = ready.getByRole("button", { name: "Add open note" }) as HTMLButtonElement;
    expect(add.disabled).toBe(false);
    fireEvent.click(add);
    await waitFor(() => expect(mocks.addOpenNote).toHaveBeenCalledTimes(1));
    cleanup();

    const unsaved = mount({ openNote: "unsaved" });
    const blocked = unsaved.getByRole("button", { name: "Add open note" }) as HTMLButtonElement;
    expect(blocked.disabled).toBe(true);
    expect(unsaved.getByText("Save this note first")).toBeTruthy();
  });

  it("names a chip by its folder and keeps the whole key within reach", () => {
    vi.useFakeTimers();
    mocks.attachments = [chip("Notes/Ideas/Launch.md")];
    const { container } = mount();
    expect(container.querySelector(".chat-chip-name")?.textContent).toBe("…/Ideas/Launch.md");
    expect(container.querySelector(".chat-chip-remove")?.getAttribute("aria-label")).toBe(
      "Remove Launch.md",
    );
    expect(tipOver(container)).toBe("Notes/Ideas/Launch.md");
  });

  // The automatic chip reads and removes like every other: the store decides
  // what a removal means for the tab it came from.
  it("hands a removed chip to the store, automatic or not", () => {
    mocks.attachments = [chip("Launch.md", { auto: true })];
    const { container } = mount();

    fireEvent.click(container.querySelector(".chat-chip-remove") as HTMLElement);

    expect(mocks.detach).toHaveBeenCalledWith("Launch.md");
  });

  it("says a dirty note sends its saved text", () => {
    vi.useFakeTimers();
    mocks.attachments = [chip("Launch.md", { dirty: true })];
    const { container } = mount();
    expect(tipOver(container)).toBe("Sends the saved version of Launch.md");
  });

  it("blocks Send while a chip cannot be read", () => {
    mocks.draft = "hello";
    mocks.attachments = [
      chip("Archive/Gone.md", { state: "unreadable", reason: "This note is no longer there." }),
    ];
    const { container, getByRole } = mount();
    expect(container.querySelector(".chat-chip.is-unreadable")).toBeTruthy();
    expect(getByRole("button", { name: "Send" }).hasAttribute("disabled")).toBe(true);
    // The line names the note by its whole key, since a bare name can belong
    // to a note in another folder, and the reason says only what is wrong.
    expect(container.textContent).toContain("Archive/Gone.md: This note is no longer there.");
  });
});

describe("the mention list", () => {
  async function openList(hits: { path: string; name: string; folder?: string }[]) {
    mocks.candidates.mockResolvedValue(hits);
    const view = mount();
    const el = field(view.container);
    el.value = "see @la";
    fireEvent.input(el);
    await waitFor(() => expect(view.container.querySelector(".chat-mention-row")).toBeTruthy());
    return { ...view, el };
  }

  it("points the field at the active row", async () => {
    const { container, el } = await openList([
      { path: "Launch.md", name: "Launch.md" },
      { path: "Later.md", name: "Later.md" },
    ]);

    expect(el.getAttribute("role")).toBe("combobox");
    expect(el.getAttribute("aria-expanded")).toBe("true");
    const list = container.querySelector(".chat-mention-list") as HTMLElement;
    expect(el.getAttribute("aria-controls")).toBe(list.id);
    expect(el.getAttribute("aria-activedescendant")).toBe("chat-mention-0");

    fireEvent.keyDown(el, { key: "ArrowDown" });
    expect(el.getAttribute("aria-activedescendant")).toBe("chat-mention-1");

    const rows = Array.from(container.querySelectorAll(".chat-mention-row"));
    expect(rows.every((row) => row.getAttribute("tabindex") === "-1")).toBe(true);
  });

  it("names the folder of a note that shares its name with another", async () => {
    const { container } = await openList([
      { path: "/n/Launch.md", name: "Launch.md", folder: "" },
      { path: "/n/Archive/Launch.md", name: "Launch.md", folder: "Archive" },
    ]);

    const rows = Array.from(container.querySelectorAll(".chat-mention-row"));
    expect(rows[0].querySelector(".chat-mention-folder")).toBeNull();
    expect(rows[1].querySelector(".chat-mention-folder")?.textContent).toBe("Archive");
  });

  it("keeps the active row in view", async () => {
    const seen: unknown[] = [];
    const scroll = vi.fn(function (this: Element, arg: unknown) {
      seen.push([this.textContent, arg]);
    });
    Object.defineProperty(Element.prototype, "scrollIntoView", {
      value: scroll,
      configurable: true,
      writable: true,
    });

    const { el } = await openList([
      { path: "Launch.md", name: "Launch.md" },
      { path: "Later.md", name: "Later.md" },
    ]);
    fireEvent.keyDown(el, { key: "ArrowDown" });

    await waitFor(() => expect(scroll).toHaveBeenCalled());
    expect(seen[seen.length - 1]).toEqual(["Later.md", { block: "nearest" }]);
  });

  it("keeps the empty answer out of the list", async () => {
    mocks.candidates.mockResolvedValue([]);
    const { container } = mount();
    const el = field(container);
    el.value = "see @zz";
    fireEvent.input(el);

    await waitFor(() => expect(container.textContent).toContain("No note by that name."));
    const list = container.querySelector(".chat-mention-list") as HTMLElement;
    expect(list.textContent).not.toContain("No note by that name.");
  });
});
