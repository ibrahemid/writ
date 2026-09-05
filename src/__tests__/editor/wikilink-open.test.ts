import { describe, it, expect, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { linkLayer, type LinkDeps } from "../../editor/link-layer";
import {
  followNoteLink,
  type NoteLinkActions,
  type NoteLinkResolution,
} from "../../editor/wikilink-open";
import { wikilinkName } from "../../lib/wikilink";
import { IS_MAC } from "../../lib/platform";

const NOWHERE: NoteLinkResolution = { status: "missing", path: null, candidates: [] };

function actionsFor(resolution: NoteLinkResolution, createdPath = "/notes/New.md") {
  const actions: NoteLinkActions & Record<string, unknown> = {
    resolve: vi.fn(async () => resolution),
    openPath: vi.fn(),
    showCandidates: vi.fn(),
    offerCreate: vi.fn(),
    create: vi.fn(async () => createdPath),
  };
  return actions;
}

describe("wikilinkName", () => {
  it("strips the alias, the heading and the folder", () => {
    expect(wikilinkName("Note")).toBe("Note");
    expect(wikilinkName("Note|alias")).toBe("Note");
    expect(wikilinkName("Note#Heading")).toBe("Note");
    expect(wikilinkName("folder/Note")).toBe("Note");
    expect(wikilinkName("folder/Note#Heading|alias")).toBe("Note");
  });
});

describe("followNoteLink", () => {
  it("opens the note a resolved target names", async () => {
    const actions = actionsFor({
      status: "resolved",
      path: "/notes/Target.md",
      candidates: [],
    });
    await followNoteLink("/notes/From.md", "Target", actions);
    expect(actions.resolve).toHaveBeenCalledWith("/notes/From.md", "Target");
    expect(actions.openPath).toHaveBeenCalledWith("/notes/Target.md");
    expect(actions.offerCreate).not.toHaveBeenCalled();
    expect(actions.showCandidates).not.toHaveBeenCalled();
  });

  it("shows the candidates of an ambiguous target and opens the one picked", async () => {
    const actions = actionsFor({
      status: "ambiguous",
      path: null,
      candidates: ["/notes/a/Note.md", "/notes/b/Note.md"],
    });
    await followNoteLink("/notes/From.md", "Note", actions);
    expect(actions.showCandidates).toHaveBeenCalledWith(
      "Note",
      ["/notes/a/Note.md", "/notes/b/Note.md"],
      expect.any(Function),
    );
    expect(actions.openPath).not.toHaveBeenCalled();

    const onPick = vi.mocked(actions.showCandidates).mock.calls[0][2];
    onPick("/notes/b/Note.md");
    expect(actions.openPath).toHaveBeenCalledWith("/notes/b/Note.md");
  });

  it("offers to create a note the target names, and creating it opens it", async () => {
    const actions = actionsFor(NOWHERE);
    await followNoteLink("/notes/From.md", "folder/New|alias", actions);
    expect(actions.offerCreate).toHaveBeenCalledWith("New", expect.any(Function));
    expect(actions.create).not.toHaveBeenCalled();

    const onCreate = vi.mocked(actions.offerCreate).mock.calls[0][1];
    onCreate();
    await Promise.resolve();
    await Promise.resolve();
    expect(actions.create).toHaveBeenCalledWith("New");
    expect(actions.openPath).toHaveBeenCalledWith("/notes/New.md");
  });

  it("does nothing for a target with nothing in it", async () => {
    const actions = actionsFor(NOWHERE);
    await followNoteLink("/notes/From.md", "   ", actions);
    expect(actions.resolve).not.toHaveBeenCalled();
    expect(actions.offerCreate).not.toHaveBeenCalled();
  });
});

// The whole gesture: the layer finds the target, hands it on, and the answer
// decides what opens.
describe("a modifier click on a wikilink", () => {
  it("opens the note the target resolved to", async () => {
    const actions = actionsFor({
      status: "resolved",
      path: "/notes/Target.md",
      candidates: [],
    });
    const deps: LinkDeps = {
      openUrl: vi.fn(),
      openWorkspaceFile: vi.fn(),
      openNoteLink: (target) => {
        void followNoteLink("/notes/From.md", target, actions);
      },
    };
    const view = new EditorView({
      state: EditorState.create({
        doc: "see [[Target]] now",
        extensions: [markdown({ base: markdownLanguage }), linkLayer(deps)],
      }),
      parent: document.body,
    });
    view.posAtCoords = ((): number | null => 8) as EditorView["posAtCoords"];
    view.contentDOM.dispatchEvent(
      new MouseEvent("mousedown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        metaKey: IS_MAC,
        ctrlKey: !IS_MAC,
      }),
    );

    await Promise.resolve();
    await Promise.resolve();
    expect(actions.openPath).toHaveBeenCalledWith("/notes/Target.md");
    expect(deps.openWorkspaceFile).not.toHaveBeenCalled();
    view.destroy();
  });
});
