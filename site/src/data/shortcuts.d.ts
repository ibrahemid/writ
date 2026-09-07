declare module '*/shortcuts.json' {
  interface SiteShortcut {
    id: string;
    label: string;
    keybinding: string;
    aliases?: string[];
  }
  interface SiteShortcutSheet {
    /** App version whose chords these are; a later version means a stale sheet. */
    frozenThrough: string;
    keys: SiteShortcut[];
  }
  const value: SiteShortcutSheet;
  export default value;
}
