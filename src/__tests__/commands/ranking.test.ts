import { describe, it, expect } from "vitest";
import {
  partitionEmptyQuery,
  rankWithQuery,
  RECENCY_HALF_LIFE_MS,
  RECENT_SECTION_LIMIT,
  RECENT_SECTION_WINDOW_MS,
  usageBoost,
} from "../../commands/ranking";
import type { Command } from "../../types/commands";
import type { CommandUsage } from "../../types/config";

const NOW = 1_715_000_000_000;

function cmd(id: string, label: string, description?: string): Command {
  return { id, label, description, scope: "app", execute: () => {} };
}

function usage(count: number, lastUsedMs: number): CommandUsage {
  return { count, last_used_ms: lastUsedMs };
}

describe("partitionEmptyQuery", () => {
  it("partitions recent and rest with alphabetical fallback", () => {
    const commands = [
      cmd("a", "Apple"),
      cmd("b", "Banana"),
      cmd("c", "Cherry"),
      cmd("d", "Date"),
    ];
    const result = partitionEmptyQuery(
      commands,
      {
        b: usage(3, NOW - 1_000),
        d: usage(1, NOW - 10_000),
      },
      NOW,
    );
    expect(result.recent.map((c) => c.id)).toEqual(["b", "d"]);
    expect(result.rest.map((c) => c.id)).toEqual(["a", "c"]);
  });

  it("caps Recent at five entries and breaks ties alphabetically", () => {
    const commands = Array.from({ length: 7 }, (_, i) =>
      cmd(`cmd${i}`, `Command ${i}`),
    );
    const sharedTime = NOW - 1_000;
    const result = partitionEmptyQuery(
      commands,
      Object.fromEntries(commands.map((c) => [c.id, usage(1, sharedTime)])),
      NOW,
    );
    expect(result.recent).toHaveLength(RECENT_SECTION_LIMIT);
    expect(result.recent.map((c) => c.id)).toEqual([
      "cmd0",
      "cmd1",
      "cmd2",
      "cmd3",
      "cmd4",
    ]);
  });

  it("excludes commands last used outside the 30-day window", () => {
    const commands = [cmd("recent", "Recent"), cmd("stale", "Stale")];
    const result = partitionEmptyQuery(
      commands,
      {
        recent: usage(1, NOW - 60_000),
        stale: usage(50, NOW - RECENT_SECTION_WINDOW_MS - 1),
      },
      NOW,
    );
    expect(result.recent.map((c) => c.id)).toEqual(["recent"]);
    expect(result.rest.map((c) => c.id).sort()).toEqual(["stale"]);
  });

  it("returns an empty recent section when no usage exists", () => {
    const commands = [cmd("a", "Alpha"), cmd("b", "Beta")];
    const result = partitionEmptyQuery(commands, {}, NOW);
    expect(result.recent).toEqual([]);
    expect(result.rest.map((c) => c.id)).toEqual(["a", "b"]);
  });
});

describe("rankWithQuery", () => {
  it("prefers a label match over a description match", () => {
    const commands = [
      cmd("file.openTheme", "Customize", "open theme"),
      cmd("theme.customize", "Theme", "open the theme editor"),
    ];
    const ranked = rankWithQuery(commands, "theme", {}, NOW);
    expect(ranked[0].id).toBe("theme.customize");
  });

  it("boosts a recently used command above a non-recent one of equal match", () => {
    const commands = [
      cmd("a.theme", "Apple theme"),
      cmd("b.theme", "Banana theme"),
    ];
    const ranked = rankWithQuery(
      commands,
      "theme",
      { "b.theme": usage(1, NOW - 1_000) },
      NOW,
    );
    expect(ranked[0].id).toBe("b.theme");
  });

  it("saturates the frequency boost with log scaling", () => {
    const lowBoost = usageBoost(usage(1, NOW), NOW);
    const highBoost = usageBoost(usage(1_000_000, NOW), NOW);
    expect(highBoost).toBeGreaterThan(lowBoost);
    expect(highBoost - lowBoost).toBeLessThan(10);
  });

  it("uses the documented half-life decay constant", () => {
    const fresh = usageBoost(usage(1, NOW), NOW);
    const aged = usageBoost(usage(1, NOW - RECENCY_HALF_LIFE_MS), NOW);
    expect(aged).toBeCloseTo(fresh - 0.5, 2);
  });

  it("drops commands that do not match the query", () => {
    const commands = [cmd("a", "Alpha"), cmd("b", "Beta")];
    const ranked = rankWithQuery(commands, "zeta", {}, NOW);
    expect(ranked).toHaveLength(0);
  });

  it("returns commands alphabetically when the query is empty", () => {
    const commands = [cmd("b", "Banana"), cmd("a", "Apple")];
    const ranked = rankWithQuery(commands, "", {}, NOW);
    expect(ranked.map((c) => c.id)).toEqual(["a", "b"]);
  });
});
