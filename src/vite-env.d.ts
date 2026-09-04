/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Dev-only shell override for the three-platform screenshots: mac | win | linux. */
  readonly VITE_WRIT_PLATFORM?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
