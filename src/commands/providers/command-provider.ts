import { getAllCommands } from "../registry";
import { effectiveBinding } from "../keybindings";
import { partitionEmptyQuery, rankWithQuery } from "../ranking";
import { configStore } from "../../stores/global/config";
import type { Command } from "../../types/commands";
import type {
  PaletteMode,
  PaletteResult,
  PaletteResultSection,
  ResultProvider,
} from "../../components/Palette/types";

const RECENT_SECTION: PaletteResultSection = { kind: "recent", label: "Recent" };
const EDITOR_SECTION: PaletteResultSection = { kind: "all", label: "Editor" };

export interface CommandProviderOptions {
  // Command ids kept out of the list — a surface never offers its own opener.
  excludeIds?: readonly string[];
  // Lists every command under Recent / Commands / Editor when the query is
  // empty. Off for the search palette, where a full command dump is noise.
  listOnEmptyQuery?: boolean;
  order?: number;
  cap?: number;
  // Heading over the ranked results. The command palette leaves it off; the
  // search palette names the section so it reads against Files and Content.
  resultsLabel?: string | null;
}

// The palette calls `execute` directly rather than going through
// `executeCommand`, so the registry's execute listener never fires for a
// palette run. Usage is recorded here, and only here: settings, file and
// content rows are not commands and record nothing.
function toResult(cmd: Command, section?: PaletteResultSection): PaletteResult {
  return {
    id: `command:${cmd.id}`,
    label: cmd.label,
    detail: cmd.description,
    kbd: effectiveBinding(cmd.id, cmd.keybinding),
    section,
    execute: () => {
      cmd.execute();
      configStore.recordCommandUse(cmd.id);
    },
  };
}

export function createCommandProvider(options: CommandProviderOptions = {}): ResultProvider {
  const excluded = new Set(options.excludeIds ?? []);
  const resultsSection: PaletteResultSection = {
    kind: "results",
    label: options.resultsLabel ?? null,
  };

  function visibleCommands(): Command[] {
    return getAllCommands().filter(
      (cmd) =>
        (cmd.scope === "app" || cmd.scope === "editor") &&
        !excluded.has(cmd.id) &&
        (cmd.isAvailable?.() ?? true),
    );
  }

  return {
    id: "commands",
    section: "Commands",
    heading: null,
    order: options.order ?? 0,
    cap: options.cap ?? Number.POSITIVE_INFINITY,
    modes: ["all", "commands"],
    showKbd: true,
    query(q: string, _signal: AbortSignal, mode: PaletteMode): PaletteResult[] {
      const all = visibleCommands();
      const usage = configStore.config().commands.usage;
      if (!q) {
        // A bare `>` addresses this provider by name; it lists everything even
        // where the mixed empty view would stay silent.
        if (!options.listOnEmptyQuery && mode !== "commands") return [];
        // Recent may hold either scope; the rest is subdivided into app
        // commands then editor commands, each headed only when the split is
        // visible.
        const { recent, rest } = partitionEmptyQuery(all, usage);
        const appRest = rest.filter((cmd) => cmd.scope === "app");
        const editorRest = rest.filter((cmd) => cmd.scope === "editor");
        const subdivided = recent.length > 0 || editorRest.length > 0;
        const appSection: PaletteResultSection = {
          kind: "all",
          label: subdivided ? "Commands" : null,
        };
        return [
          ...recent.map((cmd) => toResult(cmd, RECENT_SECTION)),
          ...appRest.map((cmd) => toResult(cmd, appSection)),
          ...editorRest.map((cmd) => toResult(cmd, EDITOR_SECTION)),
        ];
      }
      return rankWithQuery(all, q, usage).map((cmd) => toResult(cmd, resultsSection));
    },
  };
}
