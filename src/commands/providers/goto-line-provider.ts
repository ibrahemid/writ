import { windowRegistry } from "../../stores/global/window-registry";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import type { PaletteResult, ResultProvider } from "../../components/Palette/types";

const LINE_RE = /^\d+$/;

export interface GotoLineProviderOptions {
  order?: number;
}

// The `:` prefix. Jumps the active buffer to a line; with no active buffer or a
// non-numeric argument it contributes nothing.
export function createGotoLineProvider(options: GotoLineProviderOptions = {}): ResultProvider {
  return {
    id: "line",
    section: "Go to line",
    order: options.order ?? -1,
    cap: 1,
    modes: ["line"],
    query(q: string): PaletteResult[] {
      if (!LINE_RE.test(q)) return [];
      const line = Number.parseInt(q, 10);
      if (!Number.isFinite(line) || line < 1) return [];
      const win = windowRegistry.getActive();
      const activeId = win?.tabs.activeTabId() ?? null;
      if (!win || !activeId) return [];
      const doc = bufferRegistry.activeTabs().find((b) => b.id === activeId);
      return [
        {
          id: `line:${line}`,
          label: `Go to line ${line}`,
          detail: doc?.title,
          execute: () => win.editor.requestReveal(activeId, line),
        },
      ];
    },
  };
}
