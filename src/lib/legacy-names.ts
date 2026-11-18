// Every value an earlier build spelled with the product's old name, read on
// upgrade and never written (ADR-059 decision 5). No other source file spells it.

/** The product name before ADR-059. */
export const LEGACY_PRODUCT_NAME = "Writ";

/** The custom property prefix the theme mirror saved its tokens under. */
export const LEGACY_CSS_PREFIX = "--writ-";

/** The theme mirror key; the inline script in index.html carries its own copy. */
export const LEGACY_THEME_MIRROR_KEY = "writ-theme-vars-v3";

/** The pane layout key, replaced by `PANES_STORAGE_KEY`. */
export const LEGACY_PANES_STORAGE_KEY = "writ.editor.panes.v1";

/** The open-folders key, replaced by the tree expansion store's own key. */
export const LEGACY_TREE_EXPANSION_STORAGE_KEY = "writ.tree.expanded.v1";

/** The prefix of a millisecond-stamped name an earlier build gave a new file. */
export const LEGACY_AUTO_NAME_PREFIX = "writ-";

/** Activity actions an earlier server logged, by the action that replaced each. */
export const LEGACY_ACTIVITY_ACTIONS: ReadonlyMap<string, string> = new Map([
  ["open_in_writ", "open_in_jotra"],
]);

/** Config ids kept as written (ADR-059 decision 6): theme presets and an accent. */
export const KEPT_CONFIG_IDS: readonly string[] = ["writ-light", "writ-dark", "writ-blue"];

/** `action` under the name the current server logs it as. */
export function currentActivityAction(action: string): string {
  return LEGACY_ACTIVITY_ACTIONS.get(action) ?? action;
}
