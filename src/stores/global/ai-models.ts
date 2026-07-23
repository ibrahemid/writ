import type { AiPreset } from "../../types/config";

// Curated fallback model ids per provider, used when the live /models list is
// unavailable. Suggestions only — not a guarantee the id is installed or
// enabled on the account. The first entry is the zero-decision default.
const CURATED: Record<AiPreset, string[]> = {
  ollama: ["qwen3:4b", "qwen3:8b", "gemma3:4b", "llama3.2:3b"],
  groq: ["llama-3.3-70b-versatile", "llama-3.1-8b-instant"],
  gemini: ["gemini-2.5-flash", "gemini-2.5-flash-lite"],
  deepseek: ["deepseek-chat", "deepseek-reasoner"],
  openrouter: ["meta-llama/llama-3.3-70b-instruct:free", "deepseek/deepseek-chat-v3-0324:free"],
  custom: [],
};

/** Curated suggestions for a preset (empty for custom). */
export function curatedModels(preset: string): string[] {
  return CURATED[preset as AiPreset] ?? [];
}

/** The zero-decision default model for a preset (first curated, or empty). */
export function defaultModelFor(preset: string): string {
  return curatedModels(preset)[0] ?? "";
}

/** The ids to offer in the picker: the live list when present, else curated. */
export function modelOptions(preset: string, live: readonly string[]): string[] {
  return live.length > 0 ? [...live] : curatedModels(preset);
}

export interface AutoModelInput {
  preset: string;
  model: string;
  live: readonly string[];
  /** The user explicitly picked the current model this session. */
  userSelected: boolean;
}

/**
 * The model to auto-assign, or `null` to leave the choice alone. Fills an empty
 * model with the first available option, and for Ollama replaces a
 * not-installed model with the first installed one — but never overrides a
 * model the user explicitly chose.
 */
export function resolveAutoModel({ preset, model, live, userSelected }: AutoModelInput): string | null {
  const options = modelOptions(preset, live);
  if (!model.trim()) {
    return options[0] ?? null;
  }
  if (preset === "ollama" && live.length > 0 && !live.includes(model) && !userSelected) {
    return live[0];
  }
  return null;
}
