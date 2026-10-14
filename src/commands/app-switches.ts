import { createEffect } from "solid-js";
import { answerOffAppCommands, defineAppCommands, syncAppCommands } from "./app-commands";
import { offerAppsInContext } from "./app-offers";
import { rewriteCommands } from "./ai";
import { closeActivity } from "../components/Activity/ActivityPanel";
import { openSettings } from "../components/SettingsModal/SettingsModal";
import { appEntry } from "../lib/apps";
import { configStore } from "../stores/global/config";
import type { WindowState } from "../stores/window/createWindowState";

type SwitchedWindow = Pick<WindowState, "chatPanel" | "folderGraph" | "sidebar" | "tabs">;

/**
 * What the window does when an app is switched on or off (ADR-042 section 3):
 * what the app had open closes, and a command of an app that is off, which is
 * what its chord runs, opens the app's row in Settings, Apps, marked. Nothing
 * an app holds on disk is touched, so switching it back on finds it as it
 * was. The connections panel is left to its own render gate, so its open
 * state comes back with it. An app that is off is offered once when the open
 * file shows a use for it (ADR-041 section 5).
 *
 * Called once from the window's owner; every effect is disposed with it.
 */
export function followAppSwitches(win: SwitchedWindow): void {
  answerOffAppCommands((app) => openSettings("apps", appEntry(app).settingId));
  defineAppCommands("rewrite", rewriteCommands());
  createEffect(() => syncAppCommands());

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

  offerAppsInContext(win);
}
