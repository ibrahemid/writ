import { createEffect } from "solid-js";
import { syncAppCommands } from "./app-commands";
import { registerAiCommands, unregisterAiCommands } from "./ai";
import { closeActivity } from "../components/Activity/ActivityPanel";
import { configStore } from "../stores/global/config";
import type { WindowState } from "../stores/window/createWindowState";

type SwitchedWindow = Pick<WindowState, "chatPanel" | "folderGraph" | "sidebar">;

/**
 * What the window does when an app is switched on or off (ADR-042 section 3):
 * the app's commands come and go, and what it had open closes. Nothing an app
 * holds on disk is touched, so switching it back on finds it as it was. The
 * connections panel is left to its own render gate, so its open state comes
 * back with it.
 *
 * Called once from the window's owner; every effect is disposed with it.
 */
export function followAppSwitches(win: SwitchedWindow): void {
  createEffect(() => syncAppCommands((app) => configStore.isAppOn(app)));

  createEffect(() => {
    if (configStore.isAppOn("rewrite")) registerAiCommands();
    else unregisterAiCommands();
  });

  createEffect(() => {
    if (!configStore.isAppOn("chat")) win.chatPanel.hide();
  });
  createEffect(() => {
    if (!configStore.isAppOn("graph")) win.folderGraph.close();
  });
  createEffect(() => {
    if (!configStore.isAppOn("programs")) closeActivity();
  });
  createEffect(() => {
    if (!configStore.isAppOn("tags")) win.sidebar.selectTag(null);
  });
}
