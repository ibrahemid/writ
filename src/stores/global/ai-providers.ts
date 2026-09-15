import { createSignal, createRoot } from "solid-js";
import { aiProviders, type AiProviderGroup, type AiProviderInfo } from "../../services/tauri";

export type { AiProviderGroup, AiProviderInfo };

/** The heading each group carries in the picker, in the order they are shown:
 * what already runs on the machine first. */
export const PROVIDER_GROUP_LABELS: Record<AiProviderGroup, string> = {
  local: "Running on this machine",
  hosted: "Hosted",
  custom: "Custom (OpenAI-compatible)",
};

export const PROVIDER_GROUP_ORDER: AiProviderGroup[] = ["local", "hosted", "custom"];

export interface ProviderGroupedOptions {
  group: AiProviderGroup;
  label: string;
  providers: AiProviderInfo[];
}

/** Groups the table for the picker, dropping a group the table does not fill. */
export function groupProviders(rows: readonly AiProviderInfo[]): ProviderGroupedOptions[] {
  return PROVIDER_GROUP_ORDER.map((group) => ({
    group,
    label: PROVIDER_GROUP_LABELS[group],
    providers: rows.filter((row) => row.group === group),
  })).filter((entry) => entry.providers.length > 0);
}

// Singleton state — Writ is single-window. The table is data Rust owns and it
// does not change while the app runs, so it is fetched once and read
// synchronously afterwards by the picker, the probe and `connectionDisplay`.
function createAiProvidersStore() {
  const [rows, setRows] = createSignal<AiProviderInfo[]>([]);
  let inFlight: Promise<void> | null = null;

  async function load(): Promise<void> {
    if (rows().length > 0) return;
    if (inFlight) {
      await inFlight;
      return;
    }
    inFlight = aiProviders()
      .then((table) => {
        setRows(table);
      })
      .catch(() => {
        setRows([]);
      })
      .finally(() => {
        inFlight = null;
      });
    await inFlight;
  }

  function byId(id: string): AiProviderInfo | null {
    return rows().find((row) => row.id === id) ?? null;
  }

  return {
    rows,
    grouped: () => groupProviders(rows()),
    load,
    byId,
    /** The group a provider id belongs to, or `null` while the table is
     * unloaded or the id is not in it. */
    groupOf: (id: string): AiProviderGroup | null => byId(id)?.group ?? null,
  };
}

export type AiProvidersStore = ReturnType<typeof createAiProvidersStore>;
export const aiProvidersStore = createRoot(createAiProvidersStore);
