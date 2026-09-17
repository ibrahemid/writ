import { describe, it, expect, afterEach } from "vitest";
import { createRoot } from "solid-js";
import { render, fireEvent, cleanup } from "@solidjs/testing-library";
import { EditorView } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { search } from "@codemirror/search";
import FindOverlay from "../../components/Find/FindOverlay";
import { registerCommand, unregisterCommand } from "../../commands/registry";
import { createFindController, createEditorSurface } from "../../stores/global/find-store";

function makeView(doc: string) {
  const state = EditorState.create({ doc, extensions: [search({ top: true })] });
  const container = document.createElement("div");
  document.body.appendChild(container);
  return new EditorView({ state, parent: container });
}

function makeStore(view: EditorView) {
  let store!: ReturnType<typeof createFindController>;
  createRoot(() => {
    store = createFindController(() => createEditorSurface(() => view));
  });
  return store;
}

afterEach(cleanup);

describe("FindOverlay", () => {
  it("renders nothing while closed", () => {
    const find = makeStore(makeView("foo"));
    const { queryByRole } = render(() => <FindOverlay store={find} />);
    expect(queryByRole("search")).toBeNull();
  });

  it("shows the search field once opened", () => {
    const find = makeStore(makeView("foo foo"));
    const { getByLabelText } = render(() => <FindOverlay store={find} />);
    find.open();
    expect(getByLabelText("Find")).toBeTruthy();
  });

  it("keeps the find and replace inputs silent on a mouse click", () => {
    const find = makeStore(makeView("foo foo"));
    const { getByLabelText } = render(() => <FindOverlay store={find} />);
    find.open();
    fireEvent.click(getByLabelText("Replace"));
    expect(getByLabelText("Find").hasAttribute("data-writ-focus-silent")).toBe(true);
    expect(getByLabelText("Replace with").hasAttribute("data-writ-focus-silent")).toBe(true);
  });

  it("renders the match count and updates on navigation", () => {
    const find = makeStore(makeView("foo bar foo baz foo"));
    const { getByText, getByLabelText } = render(() => <FindOverlay store={find} />);
    find.open();
    find.setQueryText("foo");
    expect(getByText("3 matches")).toBeTruthy();

    fireEvent.click(getByLabelText("Next match"));
    expect(getByText("1 of 3")).toBeTruthy();
  });

  it("shows a no-results state", () => {
    const find = makeStore(makeView("foo"));
    const { getByText } = render(() => <FindOverlay store={find} />);
    find.open();
    find.setQueryText("zzz");
    expect(getByText("No results")).toBeTruthy();
  });

  it("reflects toggle state via aria-pressed", () => {
    const find = makeStore(makeView("Foo foo"));
    const { getByTitle } = render(() => <FindOverlay store={find} />);
    find.open();
    const caseBtn = getByTitle("Match case");
    expect(caseBtn.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(caseBtn);
    expect(caseBtn.getAttribute("aria-pressed")).toBe("true");
  });

  it("closes on Escape", () => {
    const find = makeStore(makeView("foo"));
    const { getByLabelText, queryByRole } = render(() => <FindOverlay store={find} />);
    find.open();
    fireEvent.keyDown(getByLabelText("Find"), { key: "Escape" });
    expect(find.isOpen()).toBe(false);
    expect(queryByRole("search")).toBeNull();
  });

  it("reveals the replace row when toggled", () => {
    const find = makeStore(makeView("foo foo"));
    const { getByLabelText, queryByLabelText } = render(() => <FindOverlay store={find} />);
    find.open();
    expect(queryByLabelText("Replace with")).toBeNull();
    fireEvent.click(getByLabelText("Replace"));
    expect(getByLabelText("Replace with")).toBeTruthy();
  });
});

// A button with content is named by its content, so "Aa", "ab", ".*" and "All"
// are what a screen reader and voice control get; a title never wins the
// accessible name once there is text inside.
describe("the find bar's names", () => {
  const openBar = () => {
    const find = makeStore(makeView("foo foo"));
    const view = render(() => <FindOverlay store={find} />);
    find.open();
    return { find, ...view };
  };

  it("names the three option toggles", () => {
    const { getByLabelText } = openBar();
    for (const name of ["Match case", "Whole word", "Regular expression"]) {
      expect(getByLabelText(name)).toBeTruthy();
    }
  });

  it("names the replace control after the command that opens it, with its key", () => {
    registerCommand({
      id: "editor.replace",
      label: "Replace",
      description: "Find and replace text in the current document",
      keybinding: "CmdOrCtrl+Alt+F",
      scope: "editor",
      execute: () => undefined,
    });
    try {
      const { getByLabelText } = openBar();
      expect(getByLabelText("Replace").getAttribute("title")).toMatch(/^Replace \(.+\)$/);
    } finally {
      unregisterCommand("editor.replace");
    }
  });

  it("gives every control in the first row a title", () => {
    const { container } = openBar();
    const titles = [...container.querySelectorAll(".find-row button")].map((b) =>
      b.getAttribute("title"),
    );
    expect(titles.length).toBeGreaterThan(0);
    for (const title of titles) expect(title).toBeTruthy();
  });

  it("names the replace-all button in full", () => {
    const { getByLabelText, find } = openBar();
    find.toggleReplace();
    expect(getByLabelText("Replace all")).toBeTruthy();
  });

  it("closes under one name", () => {
    const { getByLabelText } = openBar();
    expect(getByLabelText("Close").getAttribute("title")).toContain("Close");
  });
});
