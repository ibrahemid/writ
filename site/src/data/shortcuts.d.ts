declare module '*/shortcuts.json' {
  interface SiteShortcut {
    id: string;
    label: string;
    keybinding: string;
    aliases?: string[];
  }
  const value: SiteShortcut[];
  export default value;
}
