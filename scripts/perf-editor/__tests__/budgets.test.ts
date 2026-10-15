import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  AUDIT_FIGURES,
  BUDGETS,
  METRICS,
  SCENARIOS,
  TOGGLE_BUDGETS,
  filterTasksStartedIn,
  maximum,
  median,
  overBudget,
  percentile,
  type MetricKey,
} from "../budgets.mjs";
import { classifySize } from "../../../demo/backend/classify";

const ROOT = process.cwd();
const doc = (path: string) => readFileSync(join(ROOT, path), "utf8");

/** The rows of the markdown table between `<!-- name:start -->` and `<!-- name:end -->`, cells trimmed. */
function tableRows(markdown: string, name: string): string[][] {
  const start = markdown.indexOf(`<!-- ${name}:start -->`);
  const end = markdown.indexOf(`<!-- ${name}:end -->`);
  if (start === -1 || end === -1) throw new Error(`docs carry no ${name} table`);
  return markdown
    .slice(start, end)
    .split("\n")
    .filter((line) => line.startsWith("|") && !/^\|\s*-/.test(line))
    .slice(1)
    .map((line) =>
      line
        .slice(1, -1)
        .split("|")
        .map((cell) => cell.trim()),
    );
}

const within = Object.fromEntries(METRICS.map((metric) => [metric.key, 1])) as Record<MetricKey, number>;

describe("editor perf gate statistics", () => {
  it("takes the nearest-rank percentile the audit reported", () => {
    const twenty = Array.from({ length: 20 }, (_, i) => 20 - i);
    expect(percentile(twenty, 95)).toBe(20);
    expect(percentile(twenty, 50)).toBe(11);
    expect(median([3, 1, 2])).toBe(2);
    expect(median([4, 1, 3, 2])).toBe(3);
    expect(maximum([3, 9, 1])).toBe(9);
  });

  it("has no statistic for no samples", () => {
    expect(median([])).toBeNull();
    expect(percentile([], 95)).toBeNull();
    expect(maximum([])).toBeNull();
  });
});

describe("editor perf gate long tasks", () => {
  const from = 1_000;
  const to = 4_500;

  it("counts only the long tasks that started inside the window", () => {
    const tasks = [
      { start: 880, duration: 190 },
      { start: 1_000, duration: 60 },
      { start: 2_300, duration: 75 },
      { start: 4_500, duration: 120 },
    ];
    expect(filterTasksStartedIn(tasks, from, to)).toEqual([
      { start: 1_000, duration: 60 },
      { start: 2_300, duration: 75 },
    ]);
  });

  it("leaves out a task that started before the window and was reported inside it", () => {
    expect(filterTasksStartedIn([{ start: 900, duration: 400 }], from, to)).toEqual([]);
    expect(filterTasksStartedIn([], from, to)).toEqual([]);
  });
});

describe("editor perf gate budgets", () => {
  it("passes measurements within every budget", () => {
    expect(overBudget("prose-1MiB", within)).toEqual([]);
  });

  it("names each metric over its budget", () => {
    const limit = BUDGETS["prose-1MiB"].findSettleMs;
    expect(overBudget("prose-1MiB", { ...within, findSettleMs: limit + 1 })).toEqual([
      `Find to final count ${limit + 1} ms is over ${limit} ms`,
    ]);
  });

  it("fails a metric that was not measured", () => {
    const { typingP95Ms: _skipped, ...rest } = within;
    expect(overBudget("prose-1MiB", rest)).toEqual(["Typing p95 was not measured"]);
    expect(overBudget("prose-1MiB", { ...within, heapMiB: null })).toEqual(["JS heap growth was not measured"]);
  });

  it("fails a scenario with no budget", () => {
    expect(overBudget("prose-2MiB", within)).toEqual(["prose-2MiB has no budget"]);
  });

  it("fails a metric with no budget", () => {
    const { findTypingTaskMs: _unbudgeted, ...budget } = BUDGETS["prose-1MiB"];
    expect(overBudget("prose-1MiB", within, { "prose-1MiB": budget })).toEqual([
      "Longest task typing with Find open has no budget",
    ]);
  });

  it("budgets every metric of every scenario", () => {
    for (const scenario of SCENARIOS) {
      expect(Object.keys(BUDGETS[scenario.id]).sort()).toEqual(METRICS.map((metric) => metric.key).sort());
      if (!scenario.afterAudit) expect(AUDIT_FIGURES[scenario.id], scenario.id).toBeDefined();
    }
  });

  it("names a scenario after the audit exactly when the audit has no figure for it", () => {
    for (const scenario of SCENARIOS) {
      expect(AUDIT_FIGURES[scenario.id] === undefined, scenario.id).toBe(scenario.afterAudit === true);
    }
  });

  it("is never tighter than the audit's figure for a metric Find does not drive", () => {
    for (const scenario of SCENARIOS.filter((s) => !s.afterAudit)) {
      for (const metric of METRICS.filter((m) => !m.find)) {
        const figure = AUDIT_FIGURES[scenario.id][metric.key];
        expect(figure, `${scenario.id} ${metric.key}`).toBeDefined();
        expect(BUDGETS[scenario.id][metric.key], `${scenario.id} ${metric.key}`).toBeGreaterThanOrEqual(figure!);
      }
    }
  });

  it("holds Find well under the figures the audit measured before the scan ran in slices", () => {
    for (const scenario of SCENARIOS.filter((s) => !s.afterAudit)) {
      const before = AUDIT_FIGURES[scenario.id].findKeyP50Ms!;
      expect(BUDGETS[scenario.id].findKeyMaxMs, scenario.id).toBeLessThan(before);
    }
  });

  it("opens each generated file in the tier it names", () => {
    for (const scenario of SCENARIOS.filter((s) => s.spec.kind !== "log")) {
      expect(classifySize(scenario.spec.size, false).kind, scenario.id).toBe(scenario.tier);
    }
  });

  it("states the same budgets in docs/perf-budgets.md", () => {
    const rows = tableRows(doc("docs/perf-budgets.md"), "editor-perf-budgets");
    expect(rows.map((row) => row[0])).toEqual(SCENARIOS.map((scenario) => `\`${scenario.id}\``));
    for (const [index, scenario] of SCENARIOS.entries()) {
      const [, tier, ...cells] = rows[index];
      expect(tier, scenario.id).toBe(scenario.tier);
      expect(cells.map(Number), scenario.id).toEqual(METRICS.map((metric) => BUDGETS[scenario.id][metric.key]));
    }
  });

  it("holds the toggle to the hard-fail column of docs/perf/toggle-latency-baseline.md", () => {
    const targets = doc("docs/perf/toggle-latency-baseline.md");
    const hardFail = (mode: string) => {
      const row = targets.split("\n").find((line) => line.startsWith(`| ${mode}`));
      if (!row) throw new Error(`no ${mode} row`);
      return Number(/(\d+) ms \|\s*$/.exec(row)?.[1]);
    };
    expect(hardFail("Cold")).toBe(TOGGLE_BUDGETS.coldP95Ms);
    expect(hardFail("Warm frontend")).toBe(TOGGLE_BUDGETS.warmP95Ms);
  });
});
