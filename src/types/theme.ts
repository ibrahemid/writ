/**
 * A preset's colour groups, in the ADR-030 vocabulary. The leaf named `default`
 * collapses onto the bare group name when the store flattens a preset, so
 * `fg.default` becomes the token `fg` and the custom property `--writ-fg`.
 * `tokenKey` is that rule; nothing else may build a key by hand.
 */
export interface ThemeTokens {
  bg: {
    canvas: string;
    sidebar: string;
    raised: string;
    sunken: string;
    hover: string;
    selected: string;
  };
  fg: {
    default: string;
    muted: string;
    faint: string;
  };
  border: {
    default: string;
    soft: string;
  };
  accent: {
    default: string;
    hover: string;
    fg: string;
  };
  status: {
    success: string;
    warning: string;
    error: string;
    foreground: string;
  };
  syntax: {
    keyword: string;
    string: string;
    comment: string;
    function: string;
    number: string;
    type: string;
    variable: string;
  };
}

export type ThemePolarity = "light" | "dark";

/**
 * A preset. The JSON files in `src/styles/themes/` carry exactly the groups
 * declared here: an undeclared group survives the cast at the import site and
 * its leaves reach `:root` as dead custom properties. Every leaf is a single
 * hex colour — composite values and per-OS overrides live in `design/tokens/`.
 */
export interface Theme extends ThemeTokens {
  id: string;
  name: string;
  polarity: ThemePolarity;
}

export type ThemeOverrides = Record<string, string>;

export interface ThemeConfig {
  preset: string;
  overrides: ThemeOverrides;
}

export const TOKEN_GROUPS = [
  "bg",
  "fg",
  "border",
  "accent",
  "status",
  "syntax",
] as const;

export type TokenGroup = (typeof TOKEN_GROUPS)[number];

/** Sentence-case titles for the editor's group headings. */
export const GROUP_LABELS: Readonly<Record<TokenGroup, string>> = {
  bg: "Background",
  fg: "Text",
  border: "Borders",
  accent: "Accent",
  status: "Status",
  syntax: "Syntax",
};

/**
 * The token a group's leaf resolves to. `default` is the group itself, so the
 * store, the editor and an override all address `--writ-fg` by the same name.
 */
export function tokenKey(group: string, leaf: string): string {
  return leaf === "default" ? group : `${group}.${leaf}`;
}

/**
 * The token keys a per-token override may name, in the ADR-030 vocabulary.
 * Each becomes `--writ-<key with dots as dashes>`, so this list is also the
 * set of custom properties a user is allowed to repaint.
 */
export const OVERRIDE_KEYS = [
  "bg.canvas",
  "bg.sidebar",
  "bg.raised",
  "bg.sunken",
  "bg.hover",
  "bg.selected",
  "border",
  "border.soft",
  "fg",
  "fg.muted",
  "fg.faint",
  "accent",
  "accent.hover",
  "accent.fg",
] as const;

export type OverrideKey = (typeof OVERRIDE_KEYS)[number];

/**
 * Pre-ADR-030 override keys that have a direct successor. A key outside this
 * map and outside the pass-through groups names a token the new vocabulary
 * dropped (`border.focus` is the accent, `border.pill` is the border), so it
 * has nothing to resolve to and is discarded.
 */
const OVERRIDE_MIGRATIONS: Readonly<Record<string, OverrideKey>> = {
  "surface.background": "bg.canvas",
  "surface.sunken": "bg.sidebar",
  "surface.raised": "bg.raised",
  "surface.input": "bg.sunken",
  "surface.hover": "bg.hover",
  "surface.elevated": "bg.selected",
  "foreground.default": "fg",
  "foreground.muted": "fg.muted",
  "foreground.subtle": "fg.faint",
  "border.default": "border",
  "border.soft": "border.soft",
  "accent.default": "accent",
  "accent.hover": "accent.hover",
  "accent.foreground": "accent.fg",
};

/** Groups whose keys ADR-030 leaves alone; an override on one still resolves. */
const OVERRIDE_PASSTHROUGH_GROUPS: ReadonlySet<string> = new Set(["status", "syntax"]);

const OVERRIDE_KEY_SET: ReadonlySet<string> = new Set<string>(OVERRIDE_KEYS);

/**
 * The current name for a stored override key, or `null` when the token it
 * named is gone. Already-current keys come back unchanged, so this is safe to
 * run on every load.
 */
export function migrateOverrideKey(key: string): string | null {
  if (OVERRIDE_KEY_SET.has(key)) return key;
  if (OVERRIDE_PASSTHROUGH_GROUPS.has(key.split(".")[0])) return key;
  return OVERRIDE_MIGRATIONS[key] ?? null;
}
