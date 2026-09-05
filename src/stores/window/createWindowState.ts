import { createFocusStore, type FocusStore } from "./focus-store";
import { createSidebarStore, type SidebarStore } from "./sidebar-store";
import { createEditorStore, type EditorStore } from "./editor-store";
import { createTabStore, type TabStore } from "./tab-store";
import { createLayoutStore, type LayoutStore } from "./layout-store";
import { createPreviewStore, type PreviewStore } from "./preview-store";
import { createDownloadStore, type DownloadStore } from "./download-store";
import { bufferRegistry } from "../global/buffer-registry";

export interface WindowState {
  windowId: number;
  focus: FocusStore;
  sidebar: SidebarStore;
  editor: EditorStore;
  tabs: TabStore;
  layout: LayoutStore;
  preview: PreviewStore;
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
  // yet to the downloads, and the downloads open it once its bytes arrive.
  downloads.attachOpener((path, options) => tabs.openFile(path, options));

  return {
    windowId: opts.windowId,
    focus: createFocusStore(),
    sidebar: createSidebarStore(),
    editor,
    tabs,
    layout: createLayoutStore({ windowId: opts.windowId }),
    preview: createPreviewStore({ windowId: opts.windowId }),
    downloads,
  };
}
