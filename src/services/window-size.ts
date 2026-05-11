import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { configStore } from "../stores/config";

const PERSIST_DEBOUNCE_MS = 500;

let resizeTimer: ReturnType<typeof setTimeout> | null = null;

async function readCurrentLogicalSize(): Promise<{ width: number; height: number } | null> {
  try {
    const win = getCurrentWindow();
    const size = await win.outerSize();
    const scale = await win.scaleFactor();
    return {
      width: Math.round(size.width / scale),
      height: Math.round(size.height / scale),
    };
  } catch {
    return null;
  }
}

export async function restoreWindowSize(): Promise<void> {
  const cfg = configStore.config().window;
  if (!cfg || cfg.width <= 0 || cfg.height <= 0) return;
  try {
    const win = getCurrentWindow();
    const current = await readCurrentLogicalSize();
    if (current && current.width === cfg.width && current.height === cfg.height) return;
    await win.setSize(new LogicalSize(cfg.width, cfg.height));
  } catch {}
}

export async function installWindowSizePersistence(): Promise<() => void> {
  try {
    const win = getCurrentWindow();
    const unlisten = await win.onResized(() => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(async () => {
        const current = await readCurrentLogicalSize();
        if (!current) return;
        const existing = configStore.config().window;
        if (existing && existing.width === current.width && existing.height === current.height) {
          return;
        }
        try {
          await configStore.save({
            ...configStore.config(),
            window: { width: current.width, height: current.height },
          });
        } catch {}
      }, PERSIST_DEBOUNCE_MS);
    });
    return () => {
      if (resizeTimer) clearTimeout(resizeTimer);
      unlisten();
    };
  } catch {
    return () => {};
  }
}
