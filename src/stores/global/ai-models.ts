// Curated fallback model ids per provider, used when the live list is
// unavailable. Suggestions only — not a guarantee the id is installed or
// enabled on the account. The first entry is the zero-decision default. The
// keys are the ids of the provider table (ADR-040 section 2); the local rows
// carry none, because their list is whatever the runtime has pulled.
const CURATED: Record<string, string[]> = {
  ollama: ["qwen3:4b", "qwen3:8b", "gemma3:4b", "llama3.2:3b"],
  lmstudio: [],
  anthropic: ["claude-sonnet-5", "claude-opus-5", "claude-haiku-4-5"],
  openai: ["gpt-5-mini", "gpt-5", "gpt-4.1-mini"],
  gemini: ["gemini-2.5-flash", "gemini-2.5-flash-lite"],
  openrouter: ["meta-llama/llama-3.3-70b-instruct", "openai/gpt-5-mini", "google/gemma-4-26b-a4b-it:free"],
  groq: ["llama-3.3-70b-versatile", "llama-3.1-8b-instant"],
  deepseek: ["deepseek-chat", "deepseek-reasoner"],
  mistral: ["mistral-small-latest", "mistral-large-latest"],
  xai: ["grok-4", "grok-3-mini"],
  together: ["meta-llama/Llama-3.3-70B-Instruct-Turbo"],
  fireworks: ["accounts/fireworks/models/llama-v3p3-70b-instruct"],
  custom: [],
};

/** The provider ids the curated table answers for. */
export function curatedProviderIds(): string[] {
  return Object.keys(CURATED);
}

/** Curated suggestions for a provider (empty for LM Studio and custom). */
export function curatedModels(provider: string): string[] {
  return CURATED[provider] ?? [];
}

/** The zero-decision default model for a provider (first curated, or empty).
 * The provider table's own `default_model` wins where it is loaded; this is
 * the answer before it is. */
export function defaultModelFor(provider: string): string {
  return curatedModels(provider)[0] ?? "";
}

/** The ids to offer in the picker: the live list when present, else curated. */
export function modelOptions(provider: string, live: readonly string[]): string[] {
  return live.length > 0 ? [...live] : curatedModels(provider);
}

export interface AutoModelInput {
  provider: string;
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
export function resolveAutoModel({
  provider,
  model,
  live,
  userSelected,
}: AutoModelInput): string | null {
  const options = modelOptions(provider, live);
  if (!model.trim()) {
    return options[0] ?? null;
  }
  if (provider === "ollama" && live.length > 0 && !live.includes(model) && !userSelected) {
    return live[0];
  }
  return null;
}
