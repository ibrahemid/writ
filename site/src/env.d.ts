/// <reference path="../.astro/types.d.ts" />

// Set by the deploy workflow from the UMAMI_SRC repository variable.
interface ImportMetaEnv {
  readonly PUBLIC_UMAMI_SRC?: string;
}
