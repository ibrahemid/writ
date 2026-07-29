import type { AiAction } from "../services/tauri";

/**
 * The rewrite actions, defined once.
 *
 * Commands, the status-bar chip menu, the editor context menu, the overlay's
 * title, and the unregister list all derive from this table. Before it existed
 * the set was spelled out in four places, so adding an action meant editing all
 * four and forgetting one left a stale command registered after the feature was
 * switched off.
 */
export interface RewriteActionDef {
  /** Wire id sent to the IPC command; matches `PolishAction::parse`. */
  id: AiAction;
  /** Command-registry id. */
  commandId: string;
  /** Palette label. The shared `Rewrite:` prefix is load-bearing — it is what
   * makes all five findable under one query and keeps them together in the
   * alphabetical browse instead of scattering across C/P/P/R. */
  label: string;
  /** Shorter label for menus, where the group already supplies the context. */
  menuLabel: string;
  description: string;
  /** Extra search terms, matched by the palette above descriptions. */
  keywords: string[];
}

export const REWRITE_ACTIONS: readonly RewriteActionDef[] = [
  {
    id: "proofread",
    commandId: "ai.proofread",
    label: "Rewrite: Proofread",
    menuLabel: "Proofread",
    description: "Fix spelling, grammar, and punctuation, keeping the wording",
    keywords: ["ai", "rewrite", "grammar", "spelling", "punctuation", "correct", "fix", "llm"],
  },
  {
    id: "rephrase",
    commandId: "ai.rephrase",
    label: "Rewrite: Rephrase",
    menuLabel: "Rephrase",
    description: "Restate the same meaning in different wording",
    keywords: ["ai", "rewrite", "reword", "restate", "paraphrase", "different", "llm"],
  },
  {
    id: "polish",
    commandId: "ai.polish",
    label: "Rewrite: Polish",
    menuLabel: "Polish",
    description: "Tighten and smooth while keeping the meaning and voice",
    keywords: ["ai", "rewrite", "tighten", "smooth", "improve", "edit", "tone", "llm"],
  },
  {
    id: "improve_prompt",
    commandId: "ai.improvePrompt",
    label: "Rewrite: Improve prompt",
    menuLabel: "Improve prompt",
    description: "Rewrite the text as a clearer instruction for a model, keeping placeholders",
    keywords: ["ai", "rewrite", "prompt", "improve", "instruction", "llm", "placeholder", "template"],
  },
  {
    id: "custom",
    commandId: "ai.custom",
    label: "Rewrite: Custom…",
    menuLabel: "Custom…",
    description: "Rewrite the selection with your own instruction",
    keywords: ["ai", "rewrite", "custom", "instruction", "own", "llm"],
  },
];

export const REWRITE_COMMAND_IDS: readonly string[] = REWRITE_ACTIONS.map((a) => a.commandId);

const BY_ID = new Map(REWRITE_ACTIONS.map((a) => [a.id, a]));

/** Menu-length label for an action; falls back to the wire id so an unknown
 * action can never render as blank. */
export function rewriteMenuLabel(action: AiAction): string {
  return BY_ID.get(action)?.menuLabel ?? action;
}

/** Title shown over the rewrite preview. */
export function rewriteActionLabel(action: AiAction): string {
  return BY_ID.get(action)?.menuLabel ?? action;
}
