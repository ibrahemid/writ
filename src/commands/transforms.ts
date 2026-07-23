import { applyTransform, listTransforms } from "../services/tauri";
import { windowRegistry } from "../stores/global/window-registry";
import { registerCommand } from "./registry";

export async function registerTransformCommands(): Promise<void> {
  const descriptors = await listTransforms();
  for (const descriptor of descriptors) {
    registerCommand({
      id: `transform.${descriptor.id}`,
      label: `Text: ${descriptor.metadata.label}`,
      description: descriptor.metadata.description,
      scope: "app",
      execute: () => {
        const win = windowRegistry.getActive();
        if (!win) return;
        void win.editor.applyEditToActiveBuffer({
          useSelectionIfPresent: true,
          transform: (input) => applyTransform(descriptor.id, input),
        });
      },
    });
  }
}
