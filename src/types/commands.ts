import type { IconName } from "../components/Icon/sprite.generated";
import type { AppId } from "./config";

export type CommandScope = "app" | "editor";

export interface Command {
  id: string;
  label: string;
  /**
   * The label under a menu heading that already names the family, for a
   * command whose palette label carries that family as a prefix
   * ("Transform: Trim trailing spaces" sits as "Trim trailing spaces" under
   * Transform). Omitted means the menus show `label`.
   */
  menuLabel?: string;
  description?: string;
  /**
   * Extra search terms for the palette, for words a user would reasonably type
   * that appear in neither the label nor the description. Ranked above the
   * description and below the id. Mirrors the `keywords` field the settings
   * index already uses.
   */
  keywords?: string[];
  /** Leading glyph in the palette, for commands with an obvious one. */
  icon?: IconName;
  keybinding?: string;
  keybindingAliases?: string[];
  scope: CommandScope;
  /**
   * App-scoped commands fire even while the user is typing in the editor or a
   * text input only when this is true. Editor-scoped commands are always
   * delivered to a focused editor regardless of this flag.
   */
  global?: boolean;
  /**
   * Whether the command can act right now. A command that answers `false` is
   * kept out of the palette, so a surface never offers an action it would
   * only stop: `note.delete` on a file the user opened from somebody else's
   * folder is not a Delete Writ may perform. Omitted means always available,
   * which is every command that does not depend on what is open.
   */
  isAvailable?: () => boolean;
  /**
   * The app the command belongs to. While that app is off the command is left
   * out of the palette, both menus and the shortcut editor, and stays
   * registered so its chord can still answer (ADR-042 section 3).
   * `defineAppCommands` sets it, and what the command does while the app is
   * off: open the app's switch in Settings.
   */
  app?: AppId;
  /**
   * Returns `false` when the command declined to act (no view, read-only, or a
   * no-op), so the dispatcher lets the keystroke fall through to CodeMirror and
   * the browser. Any other return value counts as handled — `void` for the
   * common case, or a `Promise` for the fire-and-forget async handlers, which a
   * plain `boolean | void` would reject.
   */
  execute: () => boolean | void | Promise<unknown>;
}
