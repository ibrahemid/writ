import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createRoot, createSignal } from "solid-js";
import { EditorSelection, EditorState, Prec, StateEffect } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { autocompletion, completionStatus, startCompletion } from "@codemirror/autocomplete";
import type { DictationEvent, DictationSession } from "../../types/dictation";

const h = vi.hoisted(() => ({
  start: vi.fn(),
  stop: vi.fn(),
  cancel: vi.fn(),
  openMicSettings: vi.fn(),
  showToast: vi.fn(),
  logFailure: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  dictationStart: h.start,
  dictationStop: h.stop,
  dictationCancel: h.cancel,
  dictationOpenMicSettings: h.openMicSettings,
}));
vi.mock("../../stores/global/notifications", () => ({ showToast: h.showToast }));
vi.mock("../../lib/log", () => ({ logFailure: h.logFailure }));
vi.mock("../../lib/platform", () => ({ IS_MAC: true }));

import {
  createDictationStore,
  FILE_IS_READ_ONLY,
  NO_FILE_TO_DICTATE_INTO,
  START_FAILED,
  STOPPED_ON_FILE_RELOAD,
  STOPPED_ON_READ_ONLY,
  STOPPED_ON_TAB_CHANGE,
  settingForFailure,
  type DictationStore,
} from "../../stores/window/dictation-store";
import { popModal, pushModal, resetModalStack } from "../../lib/modal-stack";
import { findStore } from "../../stores/global/find-store";
import {
  hideContextMenu,
  showAnchoredMenu,
  showContextMenu,
  type MenuItem,
} from "../../components/ContextMenu/ContextMenu";

type Send = (event: DictationEvent) => void;

interface Harness {
  store: DictationStore;
  view: EditorView | null;
  setBuffer: (id: string) => void;
  /** The pane's active tab, which moves before the editor loads it. */
  setActive: (id: string) => void;
  dispose: () => void;
}

const views: EditorView[] = [];

function makeView(doc: string, readOnly = false): EditorView {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({
    state: EditorState.create({
      doc,
      selection: EditorSelection.cursor(doc.length),
      extensions: EditorState.readOnly.of(readOnly),
    }),
    parent,
  });
  views.push(view);
  return view;
}

function harness(view: EditorView | null): Harness {
  const [buffer, setBuffer] = createSignal<string | null>("tab-1");
  const [active, setActive] = createSignal<string | null>("tab-1");
  let store!: DictationStore;
  let dispose!: () => void;
  createRoot((d) => {
    dispose = d;
    store = createDictationStore({
      editor: { forPane: () => ({ getView: () => view, currentBufferId: buffer }) },
      panes: { focused: () => "left", activeIn: active },
    });
  });
  return {
    store,
    view,
    setBuffer: (id) => {
      setActive(id);
      setBuffer(id);
    },
    setActive: (id) => setActive(id),
    dispose,
  };
}

function escape(target: EventTarget, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

function setVisibility(state: DocumentVisibilityState): void {
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => state });
  document.dispatchEvent(new Event("visibilitychange"));
}

/** Answers the next start with `session` once `release` is called, and hands
 * back the channel the store listens on. */
function deferredStart(sessionId = 7) {
  let send!: Send;
  let release!: () => void;
  let reject!: (reason: unknown) => void;
  h.start.mockImplementationOnce((onEvent: Send) => {
    send = onEvent;
    return new Promise<DictationSession>((resolve, fail) => {
      release = () => resolve({ session_id: sessionId, engine: "api", locale: null });
      reject = fail;
    });
  });
  return {
    send: (event: DictationEvent) => send(event),
    release: () => release(),
    reject: (reason: unknown) => reject(reason),
  };
}

/** A start that answers at once. */
function answeredStart(sessionId = 7) {
  let send!: Send;
  h.start.mockImplementationOnce(async (onEvent: Send) => {
    send = onEvent;
    return { session_id: sessionId, engine: "api", locale: null };
  });
  return (event: DictationEvent) => send(event);
}

const text = (view: EditorView) => view.state.doc.toString();
let current: Harness | null = null;

beforeEach(() => {
  h.stop.mockResolvedValue(undefined);
  h.cancel.mockResolvedValue(undefined);
  h.openMicSettings.mockResolvedValue(undefined);
});

afterEach(() => {
  current?.dispose();
  current = null;
  for (const view of views.splice(0)) {
    view.dom.parentElement?.remove();
    view.destroy();
  }
  vi.useRealTimers();
  vi.clearAllMocks();
  h.start.mockReset();
  resetModalStack();
  Reflect.deleteProperty(document, "visibilityState");
});

function use(view: EditorView | null): Harness {
  current = harness(view);
  return current;
}

describe("starting", () => {
  it("asks for an open file when the pane has none, and records nothing", async () => {
    const { store } = use(null);
    await store.toggle();
    expect(h.showToast).toHaveBeenCalledWith(NO_FILE_TO_DICTATE_INTO, "info");
    expect(h.start).not.toHaveBeenCalled();
    expect(store.phase()).toBe("idle");
  });

  it("refuses a read-only file without asking for the microphone", async () => {
    const { store } = use(makeView("locked", true));
    await store.toggle();
    expect(h.showToast).toHaveBeenCalledWith(FILE_IS_READ_ONLY, "info");
    expect(h.start).not.toHaveBeenCalled();
  });

  it("goes from starting to recording and counts the seconds from there", async () => {
    vi.useFakeTimers();
    const { store } = use(makeView("Hi"));
    const start = deferredStart();
    const started = store.toggle();
    expect(store.phase()).toBe("starting");
    expect(store.isLive()).toBe(true);
    start.release();
    await started;
    start.send({ kind: "recording" });
    expect(store.phase()).toBe("recording");
    expect(store.elapsedSeconds()).toBe(0);
    vi.advanceTimersByTime(3_000);
    expect(store.elapsedSeconds()).toBe(3);
  });

  it("shows the model download before recording", async () => {
    const { store } = use(makeView(""));
    const send = answeredStart();
    await store.toggle();
    send({ kind: "preparing", progress: 0.4 });
    expect(store.phase()).toBe("preparing");
    expect(store.progress()).toBe(0.4);
    send({ kind: "recording" });
    expect(store.progress()).toBeNull();
  });

  it("shows Rust's failure with the action it names", async () => {
    const { store } = use(makeView(""));
    const open = vi.fn();
    store.attachSettingsOpener(open);
    h.start.mockRejectedValueOnce({
      code: "key_required",
      message: "Add an API key for dictation.",
      action: "open_dictation_settings",
    });
    await store.toggle();
    expect(store.phase()).toBe("idle");
    const request = h.showToast.mock.calls[0][0];
    expect(request.text).toBe("Add an API key for dictation.");
    expect(request.type).toBe("error");
    expect(request.action.label).toBe("Open Dictation settings");
    request.action.run();
    expect(open).toHaveBeenCalledWith("dictation.api_key");
  });

  it("offers System Settings for a microphone refusal", async () => {
    const { store } = use(makeView(""));
    h.start.mockRejectedValueOnce({
      code: "mic_denied",
      message: "Writ can't use the microphone.",
      action: "open_mic_settings",
    });
    await store.toggle();
    const request = h.showToast.mock.calls[0][0];
    expect(request.action.label).toBe("Open System Settings");
    request.action.run();
    expect(h.openMicSettings).toHaveBeenCalled();
  });

  it("says the start failed when the refusal carries no failure", async () => {
    const { store } = use(makeView(""));
    h.start.mockRejectedValueOnce("channel closed");
    await store.toggle();
    expect(h.showToast).toHaveBeenCalledWith(START_FAILED, "error");
    expect(h.logFailure).toHaveBeenCalled();
    expect(store.isLive()).toBe(false);
  });
});

describe("a session", () => {
  it("inserts each segment at the cursor and ends on done", async () => {
    const view = makeView("Note");
    const { store } = use(view);
    const send = answeredStart();
    await store.toggle();
    send({ kind: "recording" });
    send({ kind: "level", level: 1.7 });
    expect(store.level()).toBe(1);
    send({ kind: "partial", text: "first" });
    expect(text(view)).toBe("Note");
    send({ kind: "segment", seq: 0, text: "first part." });
    send({ kind: "segment", seq: 1, text: "Second part." });
    expect(text(view)).toBe("Note first part. Second part.");
    send({ kind: "done" });
    expect(store.phase()).toBe("idle");
    expect(store.elapsedSeconds()).toBeNull();
  });

  it("stops listening on the second press and keeps inserting what was heard", async () => {
    const view = makeView("");
    const { store } = use(view);
    const send = answeredStart(3);
    await store.toggle();
    send({ kind: "recording" });
    await store.toggle();
    expect(h.stop).toHaveBeenCalledWith(3);
    expect(store.phase()).toBe("stopping");
    send({ kind: "transcribing", pending: 1 });
    expect(store.pending()).toBe(1);
    send({ kind: "segment", seq: 0, text: "Last words" });
    send({ kind: "done" });
    expect(text(view)).toBe("Last words");
    expect(store.phase()).toBe("idle");
  });

  it("sends a stop pressed before the start answered once it has", async () => {
    const { store } = use(makeView(""));
    const start = deferredStart(9);
    const started = store.toggle();
    await store.toggle();
    expect(h.stop).not.toHaveBeenCalled();
    start.release();
    await started;
    expect(h.stop).toHaveBeenCalledWith(9);
  });

  it("cancels on Escape, keeps what was inserted, and stops listening for the key", async () => {
    const view = makeView("");
    const { store } = use(view);
    const send = answeredStart(4);
    await store.toggle();
    send({ kind: "segment", seq: 0, text: "kept" });
    send({ kind: "partial", text: "dropped" });
    const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    document.body.dispatchEvent(escape);
    expect(h.cancel).toHaveBeenCalledWith(4);
    expect(escape.defaultPrevented).toBe(true);
    expect(store.phase()).toBe("idle");
    expect(text(view)).toBe("kept");
    expect(view.dom.querySelector(".cm-dictation-partial")).toBeNull();
    send({ kind: "segment", seq: 1, text: "late" });
    expect(text(view)).toBe("kept");
    h.cancel.mockClear();
    const later = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    document.body.dispatchEvent(later);
    expect(later.defaultPrevented).toBe(false);
    expect(h.cancel).not.toHaveBeenCalled();
  });

  it("cancels on Escape in the editor", async () => {
    const view = makeView("");
    const { store } = use(view);
    answeredStart(4);
    await store.toggle();
    expect(escape(view.contentDOM).defaultPrevented).toBe(true);
    expect(h.cancel).toHaveBeenCalledWith(4);
    expect(store.isLive()).toBe(false);
  });

  it("leaves Escape in a field outside the editor to that field", async () => {
    const { store } = use(makeView(""));
    answeredStart(4);
    await store.toggle();
    const field = document.createElement("input");
    document.body.appendChild(field);
    const reached = vi.fn();
    field.addEventListener("keydown", reached);
    const event = escape(field);
    field.remove();
    expect(reached).toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
    expect(h.cancel).not.toHaveBeenCalled();
    expect(store.isLive()).toBe(true);
  });

  it("leaves Escape to an open dialog", async () => {
    const view = makeView("");
    const { store } = use(view);
    answeredStart(4);
    await store.toggle();
    pushModal();
    const event = escape(view.contentDOM);
    popModal();
    expect(event.defaultPrevented).toBe(false);
    expect(h.cancel).not.toHaveBeenCalled();
    expect(store.isLive()).toBe(true);
  });

  it("leaves the Escape that ends an input method conversion alone", async () => {
    const view = makeView("");
    const { store } = use(view);
    answeredStart(4);
    await store.toggle();
    const event = escape(view.contentDOM, { keyCode: 229 });
    expect(event.isComposing).toBe(false);
    expect(event.defaultPrevented).toBe(false);
    expect(h.cancel).not.toHaveBeenCalled();
    expect(store.isLive()).toBe(true);
  });

  it("cancels on Escape from the toolbar, the sidebar, the tab strip or the other pane's editor", async () => {
    const view = makeView("");
    const other = makeView("other pane");
    const button = document.createElement("button");
    document.body.appendChild(button);
    try {
      for (const target of [button, other.contentDOM]) {
        const { store } = use(view);
        answeredStart(4);
        await store.toggle();
        expect(escape(target).defaultPrevented).toBe(true);
        expect(h.cancel).toHaveBeenCalledWith(4);
        expect(store.isLive()).toBe(false);
        current?.dispose();
        h.cancel.mockClear();
      }
    } finally {
      button.remove();
    }
  });

  it("leaves Escape to Find while it is open", async () => {
    const view = makeView("");
    const { store } = use(view);
    answeredStart(4);
    await store.toggle();
    findStore.open();
    try {
      expect(escape(view.contentDOM).defaultPrevented).toBe(false);
    } finally {
      findStore.close();
    }
    expect(h.cancel).not.toHaveBeenCalled();
    expect(store.isLive()).toBe(true);
  });

  it("leaves Escape to a menu or popover that has the focus", async () => {
    const { store } = use(makeView(""));
    answeredStart(4);
    await store.toggle();
    for (const role of ["menu", "dialog"]) {
      const layer = document.createElement("div");
      layer.setAttribute("role", role);
      const item = document.createElement("button");
      layer.appendChild(item);
      document.body.appendChild(layer);
      const event = escape(item);
      layer.remove();
      expect(event.defaultPrevented).toBe(false);
    }
    expect(h.cancel).not.toHaveBeenCalled();
    expect(store.isLive()).toBe(true);
  });

  describe("with one of the app's menus open and the focus left outside it", () => {
    const ITEMS: MenuItem[] = [{ label: "Copy", action: () => undefined }];
    const controls: HTMLElement[] = [];

    function control(): HTMLButtonElement {
      const button = document.createElement("button");
      document.body.appendChild(button);
      controls.push(button);
      return button;
    }

    afterEach(() => {
      hideContextMenu();
      for (const element of controls.splice(0)) element.remove();
    });

    it.each<[string, (view: EditorView) => HTMLElement]>([
      [
        "the editor's context menu",
        (view) => {
          showContextMenu(10, 10, ITEMS, view.scrollDOM.getBoundingClientRect());
          return view.contentDOM;
        },
      ],
      [
        "the editor's spelling popover",
        (view) => {
          showAnchoredMenu(view.contentDOM.getBoundingClientRect(), ITEMS, undefined, view.scrollDOM.getBoundingClientRect());
          return view.contentDOM;
        },
      ],
      [
        "a tab's context menu",
        () => {
          showContextMenu(10, 10, ITEMS);
          return control();
        },
      ],
      [
        "the tab strip's overflow menu",
        () => {
          const overflow = control();
          showAnchoredMenu(overflow.getBoundingClientRect(), ITEMS, overflow);
          return overflow;
        },
      ],
      [
        "a sidebar row's context menu",
        () => {
          showContextMenu(10, 10, ITEMS);
          return control();
        },
      ],
      [
        "the title bar's app menu",
        () => {
          const appMenu = control();
          showAnchoredMenu(appMenu.getBoundingClientRect(), ITEMS, appMenu);
          return appMenu;
        },
      ],
      [
        "a status bar chip's menu",
        () => {
          const chip = control();
          showAnchoredMenu(chip.getBoundingClientRect(), ITEMS, chip);
          return chip;
        },
      ],
    ])("leaves Escape to %s, then cancels once it closes", async (_menu, open) => {
      const view = makeView("");
      const { store } = use(view);
      answeredStart(4);
      await store.toggle();
      const target = open(view);
      expect(escape(target).defaultPrevented).toBe(false);
      expect(h.cancel).not.toHaveBeenCalled();
      expect(store.isLive()).toBe(true);
      hideContextMenu();
      expect(escape(target).defaultPrevented).toBe(true);
      expect(h.cancel).toHaveBeenCalledWith(4);
    });
  });

  it("leaves Escape to the editor's completion list while it is open", async () => {
    const parent = document.createElement("div");
    document.body.appendChild(parent);
    const view = new EditorView({
      state: EditorState.create({
        doc: "wo",
        selection: EditorSelection.cursor(2),
        extensions: autocompletion({ override: [() => ({ from: 0, options: [{ label: "word" }] })] }),
      }),
      parent,
    });
    views.push(view);
    const { store } = use(view);
    answeredStart(4);
    await store.toggle();
    startCompletion(view);
    await vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"));
    escape(view.contentDOM);
    expect(h.cancel).not.toHaveBeenCalled();
    expect(store.isLive()).toBe(true);
    expect(completionStatus(view.state)).toBeNull();
  });

  it("sends a cancel made before the start answered once it has", async () => {
    const { store } = use(makeView(""));
    const start = deferredStart(5);
    const started = store.toggle();
    store.cancel();
    expect(store.phase()).toBe("starting");
    expect(h.cancel).not.toHaveBeenCalled();
    start.release();
    await started;
    expect(h.cancel).toHaveBeenCalledWith(5);
    expect(store.phase()).toBe("idle");
  });

  it("holds a cancelled start until it answers, so a second press starts nothing", async () => {
    const view = makeView("");
    const { store } = use(view);
    const start = deferredStart(5);
    const started = store.toggle();
    escape(view.contentDOM);
    expect(store.phase()).toBe("starting");
    await store.toggle();
    expect(h.start).toHaveBeenCalledTimes(1);
    expect(h.stop).not.toHaveBeenCalled();
    start.release();
    await started;
    expect(h.cancel).toHaveBeenCalledWith(5);
    expect(store.phase()).toBe("idle");
    answeredStart(6);
    await store.toggle();
    expect(h.start).toHaveBeenCalledTimes(2);
  });

  it("goes idle without a message when a cancelled start fails", async () => {
    const { store } = use(makeView(""));
    const start = deferredStart(5);
    const started = store.toggle();
    store.cancel();
    start.reject({ code: "busy", message: "Busy.", action: null });
    await started;
    expect(store.phase()).toBe("idle");
    expect(h.showToast).not.toHaveBeenCalled();
  });

  it("cancels a session whose stop failed, so Rust does not keep it", async () => {
    const { store } = use(makeView(""));
    answeredStart(3);
    await store.toggle();
    h.stop.mockRejectedValueOnce("channel closed");
    await store.toggle();
    expect(h.cancel).toHaveBeenCalledWith(3);
    expect(store.isLive()).toBe(false);
  });

  it("ends when the pane shows another tab", async () => {
    const view = makeView("");
    const { store, setBuffer } = use(view);
    answeredStart(6);
    await store.toggle();
    setBuffer("tab-2");
    expect(h.cancel).toHaveBeenCalledWith(6);
    expect(h.showToast).toHaveBeenCalledWith(STOPPED_ON_TAB_CHANGE, "info");
    expect(store.isLive()).toBe(false);
  });

  it("ends once the pane moves to another tab, before the editor has loaded it", async () => {
    const view = makeView("");
    const { store, setActive } = use(view);
    answeredStart(6);
    await store.toggle();
    setActive("tab-2");
    expect(h.cancel).toHaveBeenCalledWith(6);
    expect(h.showToast).toHaveBeenCalledWith(STOPPED_ON_TAB_CHANGE, "info");
    expect(store.isLive()).toBe(false);
  });

  it("ends when the editor state under it was replaced, and says the file was reloaded", async () => {
    const view = makeView("");
    const { store } = use(view);
    const send = answeredStart(8);
    await store.toggle();
    view.setState(EditorState.create({ doc: "other" }));
    send({ kind: "segment", seq: 0, text: "words" });
    expect(text(view)).toBe("other");
    expect(h.cancel).toHaveBeenCalledWith(8);
    expect(h.showToast).toHaveBeenCalledWith(STOPPED_ON_FILE_RELOAD, "info");
    expect(store.isLive()).toBe(false);
  });

  it("says the file became read-only when that is why a segment could not land", async () => {
    const view = makeView("");
    const { store } = use(view);
    const send = answeredStart(8);
    await store.toggle();
    view.dispatch({ effects: StateEffect.appendConfig.of(Prec.highest(EditorState.readOnly.of(true))) });
    send({ kind: "segment", seq: 0, text: "words" });
    expect(text(view)).toBe("");
    expect(h.showToast).toHaveBeenCalledWith(STOPPED_ON_READ_ONLY, "info");
    expect(store.isLive()).toBe(false);
  });

  it("keeps listening while the page is out of sight; Rust decides what stops it", async () => {
    const view = makeView("");
    const { store } = use(view);
    const send = answeredStart(3);
    await store.toggle();
    send({ kind: "recording" });
    setVisibility("hidden");
    expect(h.stop).not.toHaveBeenCalled();
    expect(h.cancel).not.toHaveBeenCalled();
    expect(store.phase()).toBe("recording");
    send({ kind: "segment", seq: 0, text: "Heard" });
    expect(text(view)).toBe("Heard");
  });

  it("shows stopping once Rust has closed the microphone, and inserts what was heard", async () => {
    vi.useFakeTimers();
    const view = makeView("");
    const { store } = use(view);
    const send = answeredStart(3);
    await store.toggle();
    send({ kind: "recording" });
    vi.advanceTimersByTime(2000);
    send({ kind: "stopping" });
    expect(store.phase()).toBe("stopping");
    vi.advanceTimersByTime(5000);
    expect(store.elapsedSeconds()).toBe(2);
    send({ kind: "recording" });
    expect(store.phase()).toBe("stopping");
    await store.toggle();
    expect(h.stop).not.toHaveBeenCalled();
    send({ kind: "segment", seq: 0, text: "Heard" });
    send({ kind: "done" });
    expect(text(view)).toBe("Heard");
    expect(store.phase()).toBe("idle");
  });

  it("stays stopping when Rust confirms a stop pressed here", async () => {
    const { store } = use(makeView(""));
    const send = answeredStart(3);
    await store.toggle();
    send({ kind: "recording" });
    await store.toggle();
    send({ kind: "stopping" });
    expect(store.phase()).toBe("stopping");
    expect(h.stop).toHaveBeenCalledTimes(1);
    expect(store.isLive()).toBe(true);
  });

  it("shows an error event and goes idle; no speech is a note, not an error", async () => {
    const { store } = use(makeView(""));
    let send = answeredStart();
    await store.toggle();
    send({
      kind: "error",
      failure: { code: "network", message: "Couldn't reach the server.", action: null },
    });
    expect(store.phase()).toBe("idle");
    expect(h.showToast.mock.calls[0][0]).toMatchObject({ type: "error" });
    expect(h.showToast.mock.calls[0][0].action).toBeUndefined();
    send = answeredStart();
    await store.toggle();
    send({ kind: "error", failure: { code: "no_speech", message: "Nothing was heard.", action: null } });
    expect(h.showToast.mock.calls[1][0]).toMatchObject({ type: "info", text: "Nothing was heard." });
  });

  it("passes a notice on without ending the session", async () => {
    const { store } = use(makeView(""));
    const send = answeredStart();
    await store.toggle();
    send({ kind: "notice", message: "Clean-up was skipped." });
    expect(h.showToast).toHaveBeenCalledWith("Clean-up was skipped.", "info");
    expect(store.isLive()).toBe(true);
  });
});

describe("settingForFailure", () => {
  it("lands each fixable failure on the row that fixes it", () => {
    expect(settingForFailure("key_required")).toBe("dictation.api_key");
    expect(settingForFailure("model_not_found")).toBe("dictation.model");
    expect(settingForFailure("language_unsupported")).toBe("dictation.language");
    expect(settingForFailure("unsupported_os")).toBe("dictation.engine");
    expect(settingForFailure("consent_required")).toBe("dictation.provider");
  });
});
