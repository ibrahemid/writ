import { describe, it, expect, afterEach } from "vitest";
import { createRoot } from "solid-js";
import { render, fireEvent, cleanup } from "@solidjs/testing-library";
import { EditorView } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { search } from "@codemirror/search";
import FindOverlay from "../../components/Find/FindOverlay";
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
    expect(queryByLabelText("Replace")).toBeNull();
    fireEvent.click(getByLabelText("Toggle replace"));
    expect(getByLabelText("Replace")).toBeTruthy();
  });
});
