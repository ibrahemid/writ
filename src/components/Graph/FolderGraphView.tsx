import {
  Show,
  createEffect,
  createMemo,
  createSignal,
  createUniqueId,
  onCleanup,
  onMount,
} from "solid-js";
import GraphCanvas from "./GraphCanvas";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import { useWindow } from "../WindowProvider/WindowProvider";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import { noteFactsStore } from "../../stores/global/note-facts";
import { themeStore } from "../../stores/global/theme";
import { FOLDER_LAYOUT_OPTIONS } from "../../lib/graph/layout";
import { folderColors } from "../../lib/graph/color";
import { countMatches, folderGraphOf, matchesQuery } from "../../lib/graph/folder-graph";
import "./FolderGraphView.css";

/** The token a note in the root of the notes folder is drawn in. */
const ROOT_TOKEN = "--writ-fg-muted";

/**
 * The whole folder, drawn.
 *
 * A layer over the note rather than a pane beside it: a folder's worth of
 * notes at a panel's width is a smudge, and this is the one surface that
 * answers what the folder is shaped like. The layer covers the note; it never
 * replaces it. The preview keeps its place underneath and is hidden with
 * `hidden`, because taking a loaded preview out of the page freezes the whole
 * window on macOS (PR #127).
 *
 * Colour is the folder a note is in, taken from the accent the theme already
 * spends on links, and searching dims everything the search does not name.
 * Clicking a note opens it behind the drawing, which stays where it is: this
 * is a way of choosing what to read next, not a place to be sent away from.
 */
export default function FolderGraphView() {
  const win = useWindow();
  const activeBuffer = useActiveBuffer();
  const graph = noteFactsStore.graph();
  const graphError = noteFactsStore.graphError();
  // The folder graph is one read for the whole app; handing the hold back is
  // what stops it being re-read on every change on disk once this is closed.
  onCleanup(() => noteFactsStore.releaseGraph());

  let layer: HTMLDivElement | undefined;
  let field: HTMLInputElement | undefined;
  // What had focus when the layer took it, so closing hands it back. Every
  // other layer in the app that takes focus returns it (`focus-trap.ts`), and
  // the note is still there underneath: leaving focus on the body would mean
  // typing into nothing until the note is clicked.
  let returnFocusTo: HTMLElement | null = null;
  const titleId = createUniqueId();
  // The two colours the drawing is built from: the accent every folder's
  // colour is turned from, and what a note in no folder is drawn in. Both are
  // read off the layer, so a theme that has never been seen still paints.
  const [tokens, setTokens] = createSignal({ accent: "", root: "" });

  function readTokens() {
    const element = layer;
    if (!element) return;
    const style = getComputedStyle(element);
    setTokens({
      accent: style.getPropertyValue("--writ-accent").trim(),
      root: style.getPropertyValue(ROOT_TOKEN).trim(),
    });
  }

  const drawn = createMemo(() => {
    const rows = graph();
    return folderGraphOf(rows.nodes, rows.edges);
  });

  const matching = createMemo(() => countMatches(drawn().nodes, win.folderGraph.query()));

  const colors = createMemo(() =>
    folderColors(
      tokens().accent,
      drawn().nodes.map((node) => node.folder),
    ),
  );

  const focusPath = () => activeBuffer()?.source_path ?? "";

  /** What the drawing shows, in one line under the search. */
  const count = createMemo(() => {
    const graphNow = drawn();
    const shown = graphNow.nodes.length;
    if (shown === 0) return "No notes yet.";
    if (win.folderGraph.query().trim().length > 0) {
      return `${matching()} of ${shown} notes match`;
    }
    if (graphNow.capped) {
      return `${shown} of ${graphNow.total} notes, the largest linked group`;
    }
    return shown === 1 ? "1 note" : `${shown} notes`;
  });

  /** What the drawing is, for a reader who is never shown the drawing. */
  const description = () => {
    const shown = drawn().nodes.length;
    if (shown === 1) return "1 note in this folder and the links between them";
    return `${shown} notes in this folder and the links between them`;
  };

  /** The notes a search passed over, which the drawing draws faint. */
  const dimmed = createMemo(() => {
    const query = win.folderGraph.query();
    const paths = new Set<string>();
    if (query.trim().length === 0) return paths;
    for (const node of drawn().nodes) {
      if (!matchesQuery(node.name, query)) paths.add(node.path);
    }
    return paths;
  });

  /** What each note is drawn in: its folder's colour, or the muted one. */
  const nodeColors = createMemo(() => {
    const perFolder = colors();
    const root = tokens().root;
    const painted = new Map<string, string>();
    for (const node of drawn().nodes) {
      const color = perFolder.get(node.folder) ?? root;
      if (color) painted.set(node.path, color);
    }
    return painted;
  });

  onMount(() => {
    readTokens();
    // The layer takes focus so that Escape closes it and the arrows move the
    // drawing without anyone having to click the canvas first. What had focus
    // is the note underneath, which the layer covers, so it is kept here to be
    // handed back when the layer goes.
    const held = document.activeElement;
    returnFocusTo = held instanceof HTMLElement && held !== document.body ? held : null;
    layer?.focus();
  });

  // Closing puts focus back where it was, and in the editor when what held it
  // has gone with the palette or the menu the drawing was opened from.
  onCleanup(() => {
    if (returnFocusTo?.isConnected) returnFocusTo.focus();
    else win.editor.focusEditor();
  });

  // The accent is what every folder's colour is turned from, so a theme change
  // is a new set of colours. The read waits a microtask for the same reason
  // the canvas's does: the store writes its signal before the root's tokens.
  createEffect(() => {
    themeStore.resolvedTokens();
    queueMicrotask(readTokens);
  });

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      win.folderGraph.close();
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    // Inside the search field the arrows and the minus key are typing.
    if (event.target === field) return;
    switch (event.key) {
      case "ArrowLeft":
        win.folderGraph.panLeft();
        break;
      case "ArrowRight":
        win.folderGraph.panRight();
        break;
      case "ArrowUp":
        win.folderGraph.panUp();
        break;
      case "ArrowDown":
        win.folderGraph.panDown();
        break;
      case "+":
      case "=":
        win.folderGraph.zoomIn();
        break;
      case "-":
        win.folderGraph.zoomOut();
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  return (
    <div
      class="folder-graph"
      role="region"
      aria-labelledby={titleId}
      tabindex={-1}
      ref={layer}
      onKeyDown={onKeyDown}
    >
      <div class="folder-graph-chrome">
        <h2 class="folder-graph-title" id={titleId}>
          Graph
        </h2>
        <label class="folder-graph-search">
          <Icon name="magnifying-glass" size={14} />
          <input
            class="folder-graph-input"
            type="text"
            ref={field}
            value={win.folderGraph.query()}
            placeholder="Search notes"
            aria-label="Search notes"
            spellcheck={false}
            onInput={(event) => win.folderGraph.search(event.currentTarget.value)}
          />
        </label>
        <p class="folder-graph-count">{graphError() ?? count()}</p>
      </div>

      <Button
        class="folder-graph-close"
        variant="ghost"
        icon="x"
        aria-label="Close graph"
        onClick={() => win.folderGraph.close()}
      />

      <Show when={drawn().nodes.length > 0}>
        <GraphCanvas
          class="folder-graph-drawing"
          nodes={drawn().nodes}
          edges={drawn().edges}
          focusPath={focusPath()}
          options={FOLDER_LAYOUT_OPTIONS}
          label={description()}
          colors={nodeColors()}
          dimmed={dimmed()}
          zoom={win.folderGraph.zoom()}
          pan={win.folderGraph.pan()}
          onPanBy={(dx, dy) => win.folderGraph.panBy(dx, dy)}
          onZoomBy={(factor) => win.folderGraph.zoomBy(factor)}
          focusable
          keepsPlaces
          onOpen={(path) => void win.tabs.openFile(path).catch(() => null)}
        />
      </Show>
    </div>
  );
}
