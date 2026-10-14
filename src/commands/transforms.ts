import { textTools } from "../stores/global/text-tools";
import { windowRegistry } from "../stores/global/window-registry";
import { showToast } from "../stores/global/notifications";
import { getCommand, registerCommand } from "./registry";
import { MENU_COMMANDS } from "./menu-commands";
import type { ApplyEditResult } from "../stores/window/editor-store";

/** The palette prefix of every transform: the menus' Transform heading. */
export const TRANSFORM_PREFIX = "Transform: ";

/** The heading the menus list the transforms under. */
export const TRANSFORM_SUBMENU = "Transform";

/**
 * The transforms the menus offer, in the Edit menu's order, each with its menu
 * label. Read when a menu opens: the transforms register after the window
 * shows, so a menu opened before that lists none rather than dead rows.
 */
export function transformMenuEntries(): { commandId: string; label: string }[] {
  return MENU_COMMANDS.filter((entry) => entry.submenu === TRANSFORM_SUBMENU).flatMap((entry) => {
    const command = getCommand(entry.id);
    return command ? [{ commandId: entry.id, label: command.menuLabel ?? command.label }] : [];
  });
}

/** What a run that replaced nothing tells the person, by reason. */
export function transformOutcomeMessage(result: ApplyEditResult): string | null {
  if (result.applied) return null;
  switch (result.reason) {
    case "document-changed":
      return "The text changed before the transform finished, so nothing was replaced.";
    case "read-only":
      return "This file can't be edited.";
    case "transform-error":
      return "Could not run the transform.";
    case "no-active-view":
      return null;
  }
}

export async function registerTransformCommands(): Promise<void> {
  const descriptors = await textTools.listTransforms();
  for (const descriptor of descriptors) {
    registerCommand({
      id: `transform.${descriptor.id}`,
      label: `${TRANSFORM_PREFIX}${descriptor.metadata.label}`,
      menuLabel: descriptor.metadata.label,
      description: descriptor.metadata.description,
      keywords: ["transform", "text"],
      scope: "app",
      execute: () => {
        const win = windowRegistry.getActive();
        if (!win) return;
        void win.editor
          .applyEditToActiveBuffer({
            useSelectionIfPresent: true,
            transform: (input) => textTools.applyTransform(descriptor.id, input),
          })
          .then((result) => {
            const message = transformOutcomeMessage(result);
            if (message) showToast(message, "error");
          });
      },
    });
  }
}
