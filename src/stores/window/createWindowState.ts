import { createSidebarStore, type SidebarStore } from "./sidebar-store";
import { createRightPanelStore, type RightPanelStore } from "./right-panel-store";
import { createChatPanelStore, type ChatPanelStore } from "./chat-panel-store";
import { createEditorStore, type EditorStore } from "./editor-store";
import { createCountsStore, type CountsStore } from "./counts-store";
import { createTabStore, type TabStore } from "./tab-store";
import { createLayoutStore, type LayoutStore } from "./layout-store";
import { createPreviewStore, type PreviewStore } from "./preview-store";
import { createFolderGraphStore, type FolderGraphStore } from "./folder-graph-store";
import { createDownloadStore, type DownloadStore } from "./download-store";
import { createPaneStore, type PaneStore } from "./pane-store";
import { bufferRegistry, OpenRefusedError } from "../global/buffer-registry";

export interface WindowState {
  windowId: number;
  sidebar: SidebarStore;
  rightPanel: RightPanelStore;
  chatPanel: ChatPanelStore;
  editor: EditorStore;
  counts: CountsStore;
  tabs: TabStore;
  panes: PaneStore;
  layout: LayoutStore;
  preview: PreviewStore;
  folderGraph: FolderGraphStore;
  downloads: DownloadStore;
}

export interface CreateWindowStateOptions {
  windowId: number;
}

export function createWindowState(opts: CreateWindowStateOptions): WindowState {
  const panes = createPaneStore();
  const editor = createEditorStore({ focusedPane: panes.focused });
  // A tab leaving a pane takes its editor along to the pane it moved to, or
  // lets go of it when it only left (ADR-056).
  panes.onTabLeavesPane((from, to, id) => {
    if (to === null) editor.dropTabState(from, id);
    else editor.moveTabState(from, to, id);
  });
  const downloads = createDownloadStore();
  const tabs = createTabStore({ registry: bufferRegistry, editor, downloads, panes });
  // Each store needs the other: the tab store routes a note that is not here
  // yet to the downloads, and the downloads open it once its bytes arrive. A
  // refusal is the download pane's to show, so it reaches the store as a
  // failure rather than as a toast; Cancel on the size question is an answer.
  downloads.attachOpener(async (path, options) => {
    const outcome = await tabs.openPath(path, options);
    if (outcome.kind === "refused") throw new OpenRefusedError(outcome.path, outcome.reason);
    return outcome;
  });

  return {
    windowId: opts.windowId,
    sidebar: createSidebarStore(),
    rightPanel: createRightPanelStore(),
    chatPanel: createChatPanelStore(),
    editor,
    counts: createCountsStore(editor),
    tabs,
    panes,
    layout: createLayoutStore(),
    preview: createPreviewStore(),
    folderGraph: createFolderGraphStore(),
    downloads,
  };
}
