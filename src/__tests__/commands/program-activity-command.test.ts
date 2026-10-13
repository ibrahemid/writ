import { describe, it, expect } from "vitest";

import { rankWithQuery } from "../../commands/ranking";
import type { Command } from "../../types/commands";
import { APP_REGISTRATIONS } from "./app-registrations";

function asCommand(id: string): Command {
  const registration = APP_REGISTRATIONS.get(id);
  if (!registration) throw new Error(`${id} is not registered in App.tsx`);
  return {
    id,
    label: registration.label,
    keywords: registration.keywords,
    scope: "app",
    execute: () => {},
  };
}

describe("Program activity in the palette", () => {
  it("is named for what it lists", () => {
    expect(APP_REGISTRATIONS.get("activity.open")?.label).toBe("Program activity");
  });

  it("is found by the words a person looking for it types", () => {
    const commands = [asCommand("activity.open"), asCommand("settings.open")];
    for (const query of ["mcp", "programs", "claude", "connected"]) {
      expect(rankWithQuery(commands, query, {}).map((command) => command.id), query).toContain(
        "activity.open",
      );
    }
  });
});
