import {
  For,
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
import Tooltip from "../Tooltip/Tooltip";
import { useWindow } from "../WindowProvider/WindowProvider";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import { indexCacheStore } from "../../stores/global/index-cache";
import { notesStore } from "../../stores/global/notes";
import { themeStore } from "../../stores/global/theme";
import { FOLDER_LAYOUT_OPTIONS } from "../../lib/graph/layout";
import { folderColors } from "../../lib/graph/color";
import {
  fileGraphOf,
  folderGraphOf,
  folderLegend,
  graphMatches,
  stepMatch,
} from "../../lib/graph/folder-graph";
import { basename } from "../../lib/path";
import "./FolderGraphView.css";

/** The token a note in the root of the notes folder is drawn in. */
const ROOT_TOKEN = "--writ-fg-muted";

/** The keys the drawing takes for itself rather than handing to the search. */
const ZOOM_KEYS = new Set(["+", "=", "-"]);

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
 * spends on links, and the legend under the search names each folder's colour
 * and keeps one lit. Searching dims everything the search does not name. The
 * same layer draws the files one link from the open one, so the graph around
 * a file does not need the connections panel.
 *
 * A click chooses a note and names it; Enter or a double-click opens it and
 * closes the layer (ADR-037 section 9). From the search, the arrows step
 * through what it names and Enter opens the chosen note, or the first.
 */
export default function FolderGraphView() {
  const win = useWindow();
  const activeBuffer = useActiveBuffer();
  // The folder graph is one read for the whole app; handing the hold back is
  // what stops it being re-read on every change once this is closed.
  onCleanup(indexCacheStore.holdGraph());
  const graph = indexCacheStore.graph();
  const graphError = indexCacheStore.graphError();
  const graphSettled = indexCacheStore.graphSettled();

  let layer: HTMLDivElement | undefined;
  let field: HTMLInputElement | undefined;
  // What had focus when the layer took it, so closing hands it back. Every
  // other layer in the app that takes focus returns it (`focus-trap.ts`), and
  // the note is still there underneath: leaving focus on the body would mean
  // typing into nothing until the note is clicked.
  let returnFocusTo: HTMLElement | null = null;
  // Set when a note was opened from the drawing: what had focus belongs to the
  // note that was in front, and the one opened is where typing goes now.
  let handsFocusToEditor = false;
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

  const focusPath = () => activeBuffer()?.source_path ?? "";
  const folderName = () => {
    const root = notesStore.root();
    return root ? basename(root) : "";
  };

  const drawn = createMemo(() => {
    const rows = graph();
    return win.folderGraph.scope() === "file"
      ? fileGraphOf(rows.nodes, rows.edges, focusPath())
      : folderGraphOf(rows.nodes, rows.edges);
  });

  /** What the search names among the notes still lit, by name. */
  const matches = createMemo(() =>
    graphMatches(drawn().nodes, win.folderGraph.query(), win.folderGraph.isolated()),
  );

  const colors = createMemo(() =>
    folderColors(
      tokens().accent,
      drawn().nodes.map((node) => node.folder),
    ),
  );

  const legend = createMemo(() =>
    folderLegend(drawn().nodes, colors(), tokens().root, folderName() || "Top level"),
  );

  const nameOf = (path: string | null) =>
    path === null ? "" : (drawn().nodes.find((node) => node.path === path)?.name ?? "");

  /** What the drawing shows, in one line under the search. */
  const count = createMemo(() => {
    if (!graphSettled()) return "Reading the folder…";
    const graphNow = drawn();
    const shown = graphNow.nodes.length;
    if (win.folderGraph.scope() === "file") {
      if (focusPath() === "") return "Open a file to see the files it links with.";
      if (shown === 0) return "No links to or from this file yet.";
      const around = shown - 1;
      return around === 1 ? "1 file one link away" : `${around} files one link away`;
    }
    if (shown === 0) return "No files yet";
    if (win.folderGraph.query().trim().length > 0 || win.folderGraph.isolated() !== null) {
      return `${matches().length} of ${shown} files match`;
    }
    if (graphNow.capped) {
      return `${shown} of ${graphNow.total} files, the largest linked group`;
    }
    return shown === 1 ? "1 file" : `${shown} files`;
  });

  /** What the drawing is, for a reader who is never shown the drawing. */
  const description = () => {
    const shown = drawn().nodes.length;
    if (win.folderGraph.scope() === "file") {
      const around = Math.max(0, shown - 1);
      const name = nameOf(focusPath());
      return around === 1 ? `${name} and 1 file it links with` : `${name} and ${around} files it links with`;
    }
    if (shown === 1) return "1 file in this folder and the links between them";
    return `${shown} files in this folder and the links between them`;
  };

  /** The note a click or the arrows chose, named, with how to open it. */
  const chosenLine = () => {
    const name = nameOf(win.folderGraph.selected());
    return name ? `${name}. Enter opens it.` : "";
  };

  /** The notes a search or the legend passed over, which the drawing draws faint. */
  const dimmed = createMemo(() => {
    const paths = new Set<string>();
    if (win.folderGraph.query().trim().length === 0 && win.folderGraph.isolated() === null) {
      return paths;
    }
    const lit = new Set(matches().map((node) => node.path));
    for (const node of drawn().nodes) {
      if (!lit.has(node.path)) paths.add(node.path);
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
  // has gone with the palette or the menu the drawing was opened from, or a
  // note was opened from the drawing.
  onCleanup(() => {
    if (!handsFocusToEditor && returnFocusTo?.isConnected) returnFocusTo.focus();
    else win.editor.focusEditor();
  });

  // The accent is what every folder's colour is turned from, so a theme change
  // is a new set of colours. The read waits a microtask for the same reason
  // the canvas's does: the store writes its signal before the root's tokens.
  createEffect(() => {
    themeStore.resolvedTokens();
    queueMicrotask(readTokens);
  });

  /** Opens `path` behind the layer, then closes the layer onto it. */
  async function openAndClose(path: string) {
    await win.tabs.openFile(path).catch(() => null);
    handsFocusToEditor = true;
    win.folderGraph.close();
  }

  /** Opens the chosen note, or with `orFirst` the first the search names. */
  function openChosen(orFirst: boolean): boolean {
    const chosen = win.folderGraph.selected();
    const searched = win.folderGraph.query().trim().length > 0;
    const path = chosen ?? (orFirst && searched ? (matches()[0]?.path ?? null) : null);
    if (path === null) return false;
    void openAndClose(path);
    return true;
  }

  function onFieldKeyDown(event: KeyboardEvent) {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp": {
        event.preventDefault();
        // Stopped here: the layer's arrows move the drawing.
        event.stopPropagation();
        const step = event.key === "ArrowDown" ? 1 : -1;
        win.folderGraph.select(stepMatch(matches(), win.folderGraph.selected(), step));
        return;
      }
      case "Enter":
        event.preventDefault();
        event.stopPropagation();
        openChosen(true);
        return;
    }
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      win.folderGraph.close();
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    // Inside the search field the arrows and the minus key are typing.
    if (event.target === field) return;
    // Outside a button, a key that types goes to the search, which is what a
    // name typed at a drawing that just opened is for.
    if (event.target instanceof HTMLButtonElement) return;
    switch (event.key) {
      case "Enter":
        if (openChosen(false)) event.preventDefault();
        return;
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
        if (event.key.length !== 1 || ZOOM_KEYS.has(event.key)) return;
        win.folderGraph.search(win.folderGraph.query() + event.key);
        field?.focus();
        break;
    }
    event.preventDefault();
  }

  // The layer takes focus when it opens so Escape closes it; that focus is a
  // place to type from, not a control, so it draws no ring.
  return (
    <div
      class="folder-graph"
      data-writ-focus-silent
      role="region"
      aria-labelledby={titleId}
      tabindex={-1}
      ref={layer}
      onKeyDown={onKeyDown}
    >
      <div class="folder-graph-chrome">
        <h2 class="folder-graph-title" id={titleId}>
          {folderName() ? `Graph · ${folderName()}` : "Graph"}
        </h2>
        <div class="folder-graph-scope" role="group" aria-label="Files drawn">
          <button
            type="button"
            class="folder-graph-scope-option"
            aria-pressed={win.folderGraph.scope() === "folder"}
            onClick={() => win.folderGraph.setScope("folder")}
          >
            Folder
          </button>
          <button
            type="button"
            class="folder-graph-scope-option"
            aria-pressed={win.folderGraph.scope() === "file"}
            onClick={() => win.folderGraph.setScope("file")}
          >
            This file
          </button>
        </div>
        <label class="folder-graph-search">
          <Icon name="magnifying-glass" size={14} />
          <input
            class="folder-graph-input"
            type="text"
            ref={field}
            value={win.folderGraph.query()}
            placeholder="Search files"
            aria-label="Search files"
            spellcheck={false}
            onInput={(event) => win.folderGraph.search(event.currentTarget.value)}
            onKeyDown={onFieldKeyDown}
          />
        </label>
        <p class="folder-graph-count">{graphError() ?? count()}</p>
        <p class="folder-graph-chosen" aria-live="polite">
          {chosenLine()}
        </p>
        <Show when={legend().length > 1}>
          <ul class="folder-graph-legend" aria-label="Folders">
            <For each={legend()}>
              {(entry) => (
                <li>
                  <button
                    type="button"
                    class="folder-graph-chip"
                    aria-pressed={win.folderGraph.isolated() === entry.folder}
                    onClick={() => win.folderGraph.isolate(entry.folder)}
                  >
                    <span
                      class="folder-graph-swatch"
                      style={{ background: entry.color }}
                      aria-hidden="true"
                    />
                    {entry.label}
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>

      <div class="folder-graph-tools">
        <Tooltip label="Zoom out" chord="-">
          <Button variant="ghost" icon="minus" aria-label="Zoom out" onClick={() => win.folderGraph.zoomOut()} />
        </Tooltip>
        <Tooltip label="Zoom in" chord="++">
          <Button variant="ghost" icon="plus" aria-label="Zoom in" onClick={() => win.folderGraph.zoomIn()} />
        </Tooltip>
        <Tooltip label="Fit to window">
          <Button
            variant="ghost"
            icon="frame-corners"
            aria-label="Fit to window"
            onClick={() => win.folderGraph.resetView()}
          />
        </Tooltip>
        <Tooltip label="Close graph" chord="Escape">
          <Button
            class="folder-graph-close"
            variant="ghost"
            icon="x"
            aria-label="Close graph"
            onClick={() => win.folderGraph.close()}
          />
        </Tooltip>
      </div>

      <Show when={drawn().nodes.length > 0}>
        <Show
          when={win.folderGraph.scope() === "folder"}
          fallback={
            <GraphCanvas
              class="folder-graph-drawing"
              nodes={drawn().nodes}
              edges={drawn().edges}
              focusPath={focusPath()}
              label={description()}
              colors={nodeColors()}
              dimmed={dimmed()}
              zoom={win.folderGraph.zoom()}
              pan={win.folderGraph.pan()}
              onPanBy={(dx, dy) => win.folderGraph.panBy(dx, dy)}
              onZoomBy={(factor) => win.folderGraph.zoomBy(factor)}
              focusable
              selectedPath={win.folderGraph.selected()}
              onSelect={(path) => win.folderGraph.select(path)}
              onOpen={(path) => void openAndClose(path)}
            />
          }
        >
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
            selectedPath={win.folderGraph.selected()}
            onSelect={(path) => win.folderGraph.select(path)}
            onOpen={(path) => void openAndClose(path)}
          />
        </Show>
      </Show>
    </div>
  );
}
