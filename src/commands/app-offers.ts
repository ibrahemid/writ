import { createEffect, createMemo, onCleanup, untrack } from "solid-js";
import type { AppId } from "../types/config";
import { configStore } from "../stores/global/config";
import { bufferRegistry } from "../stores/global/buffer-registry";
import { indexCacheStore } from "../stores/global/index-cache";
import { showToast } from "../stores/global/notifications";
import { appEntry, fileShowsUseFor, OFFERED_BY_FILE } from "../lib/apps";
import { openAppSurface } from "./app-surfaces";
import type { WindowState } from "../stores/window/createWindowState";

type OfferingWindow = Pick<WindowState, "tabs">;

/** The file the window has in front, or `null` for a tab with no file. */
function activePath(win: OfferingWindow): string | null {
  const id = win.tabs.activeTabId();
  if (!id) return null;
  return bufferRegistry.activeTabs().find((tab) => tab.id === id)?.source_path ?? null;
}

/**
 * Switches `app` on from its offer and shows it, so the file that prompted
 * the offer is the first thing the app is seen with.
 */
async function takeOffer(app: AppId): Promise<void> {
  try {
    await configStore.setAppOn(app, true);
  } catch {
    showToast("Could not save your settings", "error");
    return;
  }
  openAppSurface(app);
  configStore.noteAppsOpened([app]);
}

/**
 * Offers an app that is off, once, when the open file shows a use for it
 * (ADR-041 section 5): Connections for a file with `[[links]]`, Tags for one
 * with `#tags`.
 *
 * One line with Turn on, which goes like any other notice. The offer is
 * recorded when it is shown, so dismissing it, letting it go or switching
 * files is the answer and it never comes back. One offer at a time, and none
 * at all while the switch for offers in Settings, Apps is off. The open
 * file's facts are held only while an offer is still to be made, so a person
 * who has seen both reads nothing more per tab.
 *
 * Called once from the window's owner; every effect is disposed with it.
 */
export function offerAppsInContext(win: OfferingWindow): void {
  // Nothing is offered until the person's config has been read: before then
  // the store holds a fresh install's answers, and recording an offer writes
  // the whole store back over the file.
  const waiting = createMemo<readonly AppId[]>(() =>
    configStore.loaded() && configStore.appOffersOn()
      ? OFFERED_BY_FILE.filter((app) => !configStore.isAppOn(app) && !configStore.wasAppOffered(app))
      : [],
  );
  const path = createMemo(() => (waiting().length > 0 ? activePath(win) : null));
  let showing = false;

  createEffect(() => {
    const at = path();
    if (at === null) return;
    onCleanup(indexCacheStore.holdFacts(at));
  });

  createEffect(() => {
    const at = path();
    if (at === null || !indexCacheStore.factsSettledFor(at)()) return;
    const facts = indexCacheStore.factsFor(at)();
    const app = waiting().find((each) => fileShowsUseFor(each, facts));
    if (app === undefined || showing) return;
    showing = true;
    untrack(() => {
      configStore.noteAppOffered(app);
      showToast({
        text: appEntry(app).offer!,
        type: "info",
        action: { label: "Turn on", run: () => takeOffer(app) },
        onClose: () => {
          showing = false;
        },
      });
    });
  });
}
