import { createSignal } from "solid-js";
import { logFailure } from "../../lib/log";

// The base a preview URL is built from, provided by the host at startup
// (writ-preview:// on macOS and Linux, http://writ-preview. on Windows, where
// WebView2 serves a custom scheme as http://<scheme>.<host>). The host decides
// it (TS-03/DS-03): the frontend never guesses the scheme from
// navigator.platform, because a dev build can render the Windows chrome on a
// Mac, where the engine is still WebKit.
//
// Until the host answers, previewUrl yields the inert about:blank, so the
// preview iframe parks there rather than requesting a scheme the engine may
// treat as an external navigation. The iframe element is never torn down; this
// is an src change like any other.

// Singleton state — Writ is single-window.
const [prefix, setPrefix] = createSignal<string | null>(null);

/** The inert document the iframe parks on before the host base is known. */
export const BLANK_PREVIEW_SRC = "about:blank";

/** Record the host-provided preview URL base; `null` means not known yet. */
export function setPreviewUrlPrefix(value: string | null): void {
  setPrefix(value);
}

/** Whether the host base has been loaded yet. */
export function previewUrlPrefixLoaded(): boolean {
  return prefix() !== null;
}

/**
 * Load the preview URL base once at startup from `fetchPrefix`, the host's
 * answer (App passes the service call). If the host cannot answer, the base
 * stays unknown and every pane stays parked on about:blank: a guessed scheme
 * would be wrong on one platform, and on Windows the native form is an
 * external scheme to WebView2.
 */
export async function loadPreviewUrlPrefix(fetchPrefix: () => Promise<string>): Promise<void> {
  try {
    setPrefix(await fetchPrefix());
  } catch {
    logFailure("the preview URL base could not be loaded");
  }
}

/**
 * Build a preview URL for a scope and in-scope path. Reactive: reading it in a
 * tracking scope re-runs when the host base loads, so a pane parked on
 * about:blank advances to the real blank document once it is known.
 */
export function previewUrl(scope: "document" | "chrome", path: string): string {
  const base = prefix();
  if (base === null) return BLANK_PREVIEW_SRC;
  return `${base}${scope}/${path}`;
}
