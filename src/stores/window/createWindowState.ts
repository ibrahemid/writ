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
import { bufferRegistry, OpenRefusedError } from "../global/buffer-registry";

export interface WindowState {
  windowId: number;
  sidebar: SidebarStore;
  rightPanel: RightPanelStore;
  chatPanel: ChatPanelStore;
  editor: EditorStore;
  counts: CountsStore;
  tabs: TabStore;
  layout: LayoutStore;
  preview: PreviewStore;
  folderGraph: FolderGraphStore;
  downloads: DownloadStore;
}

export interface CreateWindowStateOptions {
  windowId: number;
}

export function createWindowState(opts: CreateWindowStateOptions): WindowState {
  const editor = createEditorStore();
  const downloads = createDownloadStore();
  const tabs = createTabStore({ registry: bufferRegistry, editor, downloads });
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
    layout: createLayoutStore(),
    preview: createPreviewStore(),
    folderGraph: createFolderGraphStore(),
    downloads,
  };
}
