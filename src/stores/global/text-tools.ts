import * as api from "../../services/tauri";
import type { TransformDescriptor } from "../../types/transforms";

/** The text transforms and prompt placeholders the commands run over a selection. */
export const textTools = {
  listTransforms(): Promise<TransformDescriptor[]> {
    return api.listTransforms();
  },
  applyTransform(transformId: string, input: string): Promise<string> {
    return api.applyTransform(transformId, input);
  },
  scanPlaceholders(text: string): Promise<string[]> {
    return api.promptScanPlaceholders(text);
  },
  fillPlaceholders(text: string, values: Record<string, string>): Promise<string> {
    return api.promptFillPlaceholders(text, values);
  },
};
