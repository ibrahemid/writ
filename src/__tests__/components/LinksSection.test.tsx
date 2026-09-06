import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import LinksSection from "../../components/RightPanel/LinksSection";
import type { LinkResolution } from "../../services/tauri";

// The notes the open note links to, as text. One row per note however many
// times it is linked, and a target the index settled on nothing is listed as
// what was written rather than dropped.

interface Link {
  to_target: string;
  to_path: string | null;
  kind: string;
  line: number;
  col: number;
}

const h = vi.hoisted(() => ({
  links: [] as Link[],
  resolutions: {} as Record<string, LinkResolution>,
  openFile: vi.fn<(path: string) => Promise<{ id: string } | null>>(),
  resolveNoteLink: vi.fn<(from: string, target: string) => Promise<LinkResolution>>(),
  collapsed: new Set<string>(),
}));

vi.mock("../../stores/global/note-facts", () => ({
  noteFactsStore: {
    factsFor: () => () => ({ links: h.links, properties: [], tags: [], headings: [] }),
  },
}));

vi.mock("../../stores/global/link", () => ({
  linkStore: { resolveNoteLink: h.resolveNoteLink },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    rightPanel: {
      isCollapsed: (section: string) => h.collapsed.has(section),
      toggleSection: () => {},
    },
    tabs: { openFile: h.openFile },
  }),
}));

function link(to_target: string, to_path: string | null, line = 1): Link {
  return { to_target, to_path, kind: "wikilink", line, col: 0 };
}

function missing(): LinkResolution {
  return { status: "missing", path: null, candidates: [], heading_line: null };
}

function ambiguous(candidates: string[]): LinkResolution {
  return { status: "ambiguous", path: null, candidates, heading_line: null };
}

beforeEach(() => {
  h.links = [];
  h.resolutions = {};
  h.collapsed = new Set();
  h.openFile.mockReset().mockResolvedValue({ id: "buf-2" });
  h.resolveNoteLink
    .mockReset()
    .mockImplementation((_from, target) => Promise.resolve(h.resolutions[target] ?? missing()));
});

afterEach(cleanup);

function names(container: HTMLElement): string[] {
  return [...container.querySelectorAll(".right-panel-row-name")].map(
    (el) => el.textContent?.trim() ?? "",
  );
}

describe("the notes a note links to", () => {
  it("lists one row per note, in the order the links are written", () => {
    h.links = [
      link("Beta", "Beta.md"),
      link("notes/Gamma", "notes/Gamma.md", 3),
      link("Beta", "Beta.md", 5),
    ];
    const { container, getByText } = render(() => <LinksSection path="Alpha.md" />);
    expect(getByText("Links")).toBeTruthy();
    expect(names(container)).toEqual(["Beta", "Gamma"]);
  });

  it("opens the note a row names", () => {
    h.links = [link("Beta", "Beta.md")];
    const { getByRole } = render(() => <LinksSection path="Alpha.md" />);
    fireEvent.click(getByRole("button", { name: "Beta" }));
    expect(h.openFile).toHaveBeenCalledWith("Beta.md");
  });

  it("renders nothing at all for a note that links to nothing", () => {
    const { container, queryByText } = render(() => <LinksSection path="Alpha.md" />);
    expect(queryByText("Links")).toBeNull();
    expect(container.querySelector(".right-panel-section")).toBeNull();
  });
});

describe("a link the index settled on nothing", () => {
  it("lists a target that names no note, with nothing to open", async () => {
    h.links = [link("Not here yet", null)];
    const { container, findByText } = render(() => <LinksSection path="Alpha.md" />);
    await findByText("Not here yet");
    expect(container.querySelector("button.right-panel-row")).toBeNull();
    expect(container.querySelector(".right-panel-row-unsettled")?.textContent).toBe(
      "Not here yet",
    );
  });

  it("names the notes a target could have meant, with nothing to open", async () => {
    h.links = [link("Notes", null)];
    h.resolutions = { Notes: ambiguous(["work/Notes.md", "home/Notes.md"]) };
    const { container, findByText } = render(() => <LinksSection path="Alpha.md" />);
    await findByText("Could also mean work/Notes or home/Notes");
    expect(names(container)).toEqual(["Notes"]);
    expect(container.querySelector("button.right-panel-row")).toBeNull();
  });

  it("asks about a target once, however many times the note links to it", () => {
    h.links = [link("Notes", null), link("Notes", null, 4)];
    render(() => <LinksSection path="Alpha.md" />);
    expect(h.resolveNoteLink).toHaveBeenCalledTimes(1);
    expect(h.resolveNoteLink).toHaveBeenCalledWith("Alpha.md", "Notes");
  });
});
