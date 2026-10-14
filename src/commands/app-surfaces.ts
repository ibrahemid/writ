import { createSignal } from "solid-js";
import type { AppId } from "../types/config";
import { configStore } from "../stores/global/config";
import { windowRegistry } from "../stores/global/window-registry";
import { openActivity } from "../components/Activity/ActivityPanel";
import { OPENS_ON_FIRST_SWITCH } from "../lib/apps";

/**
 * Shows what `app` adds in the active window: the chat pane, the connections
 * panel, the graph, the sidebar that lists the tags, or the Activity window.
 * Showing, never toggling, so Open on a surface already showing leaves it.
 * Returns `false` for Rewrite, which acts on a selection and has nothing of
 * its own to show.
 */
export function openAppSurface(app: AppId): boolean {
  if (app === "programs") {
    openActivity();
    return true;
  }
  const win = windowRegistry.getActive();
  if (!win) return false;
  switch (app) {
    case "chat":
      win.chatPanel.show();
      return true;
    case "connections":
      win.rightPanel.show();
      return true;
    case "graph":
      win.folderGraph.open();
      return true;
    case "tags":
      win.sidebar.show();
      return true;
    case "rewrite":
      return false;
  }
}

/** Whether `app` has a surface Open can show. */
export function canOpenApp(app: AppId): boolean {
  return app !== "rewrite";
}

// Singleton state — Writ is single-window. The apps switched on for the first
// time from Settings, opened once Settings closes.
const [due, setDue] = createSignal<readonly AppId[]>([]);

/**
 * Switches `app` from its row in Settings.
 *
 * The first switch-on of an app whose surface sits in the window opens that
 * surface, once, when Settings closes ([`openDueApps`]): opened behind the
 * dialog it would take focus from it, and a person switching apps on reads
 * where each one went only after closing it. Switching it off again before
 * then opens nothing.
 */
export async function switchAppFromSettings(app: AppId, on: boolean): Promise<void> {
  await configStore.setAppOn(app, on);
  if (!on) {
    setDue((apps) => apps.filter((each) => each !== app));
    return;
  }
  if (!OPENS_ON_FIRST_SWITCH.has(app) || configStore.wasAppOpened(app)) return;
  setDue((apps) => (apps.includes(app) ? apps : [...apps, app]));
}

/** The apps a closing Settings will open. Reactive. */
export function dueApps(): readonly AppId[] {
  return due();
}

/**
 * Opens every app due since Settings opened, and records them in one write so
 * none opens on its own again.
 */
export function openDueApps(): void {
  const apps = due().filter((app) => configStore.isAppOn(app));
  setDue([]);
  if (apps.length === 0) return;
  for (const app of apps) openAppSurface(app);
  configStore.noteAppsOpened(apps);
}

/** Exported for tests: forgets what is due. */
export function resetDueApps(): void {
  setDue([]);
}
