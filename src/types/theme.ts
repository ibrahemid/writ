export interface ThemeTokens {
  surface: {
    background: string;
    sunken: string;
    raised: string;
    elevated: string;
    input: string;
    hover: string;
  };
  foreground: {
    default: string;
    muted: string;
    subtle: string;
  };
  border: {
    default: string;
    soft: string;
    focus: string;
    pill: string;
  };
  accent: {
    default: string;
    hover: string;
    foreground: string;
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
  "surface",
  "foreground",
  "border",
  "accent",
  "status",
  "syntax",
] as const;

export type TokenGroup = (typeof TOKEN_GROUPS)[number];
