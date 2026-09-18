import { aiProvidersStore } from "./ai-providers";

// The curated ids live in the provider table `writ-core` owns (ADR-040
// section 2), so the picker, the send preflight and the settings panel read one
// definition. They are suggestions, not an inventory: the account may not carry
// them. The first entry is the zero-decision default.

/** The provider ids the table answers for. Empty until it is loaded. */
export function curatedProviderIds(): string[] {
  return aiProvidersStore.rows().map((row) => row.id);
}

/** Curated suggestions for a provider (empty for LM Studio and custom). */
export function curatedModels(provider: string): string[] {
  return aiProvidersStore.byId(provider)?.curated_models ?? [];
}

/** The zero-decision default model for a provider: the row's own default, else
 * its first suggestion, else nothing. */
export function defaultModelFor(provider: string): string {
  const row = aiProvidersStore.byId(provider);
  return row?.default_model || row?.curated_models[0] || "";
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
 * model with the first id the provider's own list answered, and for Ollama
 * replaces a not-installed model with the first installed one — but never
 * overrides a model the user explicitly chose.
 *
 * A curated suggestion is never assigned. The picker offers the suggestions
 * while the provider's list cannot be read, but the value saved is the value a
 * send carries, and a provider can retire an id the table still names.
 */
export function resolveAutoModel({
  provider,
  model,
  live,
  userSelected,
}: AutoModelInput): string | null {
  if (!model.trim()) {
    return live[0] ?? null;
  }
  if (provider === "ollama" && live.length > 0 && !live.includes(model) && !userSelected) {
    return live[0];
  }
  return null;
}
