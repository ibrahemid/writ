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

/**
 * The model a provider starts from: the row's own default when the provider's
 * list holds it, else the first of the row's suggestions the list holds, else,
 * on a local runtime that lists one model, that model, else nothing, which
 * leaves the picker open. The rule `seed_model` applies in
 * `writ_core::ai::models` (ADR-040 section 12).
 *
 * The list's own order decides nothing: sorted, its first id is whatever sorts
 * first, which is a completion-only model on OpenAI's list and an embedding
 * model as readily as a chat model on a local runtime's.
 */
export function seedModel(provider: string, live: readonly string[]): string {
  const row = aiProvidersStore.byId(provider);
  if (!row || live.length === 0) return "";
  if (row.default_model && live.includes(row.default_model)) return row.default_model;
  const suggested = row.curated_models.find((id) => live.includes(id));
  if (suggested) return suggested;
  return row.group === "local" && live.length === 1 ? live[0] : "";
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
 * model with `seedModel`, and for Ollama replaces a not-installed model with
 * it. A model the user explicitly chose is never overridden, and an id the
 * provider's own list does not hold is never filled in.
 */
export function resolveAutoModel({
  provider,
  model,
  live,
  userSelected,
}: AutoModelInput): string | null {
  if (!model.trim()) {
    return seedModel(provider, live) || null;
  }
  if (provider === "ollama" && live.length > 0 && !live.includes(model) && !userSelected) {
    return seedModel(provider, live) || null;
  }
  return null;
}
