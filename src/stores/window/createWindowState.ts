import { createFocusStore, type FocusStore } from "./focus-store";
import { createSidebarStore, type SidebarStore } from "./sidebar-store";
import { createEditorStore, type EditorStore } from "./editor-store";
import { createTabStore, type TabStore } from "./tab-store";
import { createLayoutStore, type LayoutStore } from "./layout-store";
import { createPreviewStore, type PreviewStore } from "./preview-store";
import { bufferRegistry } from "../global/buffer-registry";

export interface WindowState {
  windowId: number;
  focus: FocusStore;
  sidebar: SidebarStore;
  editor: EditorStore;
  tabs: TabStore;
  layout: LayoutStore;
  preview: PreviewStore;
}

export interface CreateWindowStateOptions {
  windowId: number;
}

export function createWindowState(opts: CreateWindowStateOptions): WindowState {
  return {
    windowId: opts.windowId,
    focus: createFocusStore(),
    sidebar: createSidebarStore(),
    editor: createEditorStore(),
    tabs: createTabStore({ registry: bufferRegistry }),
    layout: createLayoutStore({ windowId: opts.windowId }),
    preview: createPreviewStore({ windowId: opts.windowId }),
  };
}
