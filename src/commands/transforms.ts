import { applyTransform, listTransforms } from "../services/tauri";
import { editorStore } from "../stores/editor";
import { registerCommand } from "./registry";

export async function registerTransformCommands(): Promise<void> {
  const descriptors = await listTransforms();
  for (const descriptor of descriptors) {
    registerCommand({
      id: `transform.${descriptor.id}`,
      label: `Transform: ${descriptor.metadata.label}`,
      description: descriptor.metadata.description,
      scope: "app",
      execute: () => {
        void editorStore.applyEditToActiveBuffer({
          useSelectionIfPresent: true,
          transform: (input) => applyTransform(descriptor.id, input),
        });
      },
    });
  }
}
