import { For, Show } from "solid-js";
import Kbd from "../Kbd/Kbd";
import { useCommandBinding } from "../../commands/effective-bindings";
import { MENU_COMMANDS, type MenuSection } from "../../commands/menu-commands";
import { resolvePlatform, type Platform } from "../../lib/platform";
import type { AppEntry, AppEntryPoint } from "../../lib/apps";
import "./AppEntryPoints.css";

/** The macOS menu bar's own names for the menus the shared list fills. */
const MAC_MENU_NAMES: Record<MenuSection, string> = {
  app: "Writ menu",
  file: "File menu",
  edit: "Edit menu",
  view: "View menu",
  help: "Help menu",
};

/**
 * What a place an app lives in is called on `platform`, or `null` for a chord,
 * which is drawn as keys rather than named. Windows and Linux carry every menu
 * item under the one titlebar button, which is what a person looks for there.
 */
export function entryPointName(point: AppEntryPoint, platform: Platform): string | null {
  switch (point.kind) {
    case "toolbar":
      return "Toolbar";
    case "menu": {
      if (platform !== "mac") return "Writ menu";
      const entry = MENU_COMMANDS.find((item) => item.id === point.command);
      return entry ? MAC_MENU_NAMES[entry.menu] : null;
    }
    case "place":
      return point.text;
    case "chord":
      return null;
  }
}

function ChordPoint(props: { command: string }) {
  const binding = () => useCommandBinding(props.command);
  return (
    <Show when={binding()}>
      <Kbd binding={binding()} />
    </Show>
  );
}

/**
 * Where an app lives once it is on: the toolbar, a menu, a chord or a place,
 * one line, so a person who switches it on knows where to look for it.
 */
export default function AppEntryPoints(props: { app: AppEntry; class?: string }) {
  const platform = resolvePlatform();
  return (
    <span class={`app-entry-points ${props.class ?? ""}`.trim()}>
      <For each={props.app.entryPoints}>
        {(point) =>
          point.kind === "chord" ? (
            <ChordPoint command={point.command} />
          ) : (
            <span class="app-entry-point">{entryPointName(point, platform)}</span>
          )
        }
      </For>
    </span>
  );
}
