import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  AUDIT_FIGURES,
  BUDGETS,
  FIND_RATE,
  METRICS,
  MiB,
  PAIRED_TYPING,
  SCENARIOS,
  TAB_SWITCH,
  TOGGLE_BUDGETS,
  busyMs,
  filterTasksStartedIn,
  findRate,
  holdsWholeDocumentCalls,
  maximum,
  median,
  middle,
  noiseBand,
  overBudget,
  pairedTyping,
  percentile,
  tabSwitchProblems,
  wholeDocumentCalls,
  wholeDocumentProblems,
  type CpuProfile,
  type EditorSnapshot,
  type MetricKey,
  type TypingRound,
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

describe("editor perf gate whole-document calls", () => {
  const length = 50 * MiB;

  it("holds the calls that take the whole document, whatever sends them", () => {
    const calls = [
      { kind: "Text.toString", size: length, ms: 27 },
      { kind: "TextEncoder.encode", size: length, ms: 105 },
      { kind: "JSON.stringify (the IPC send of save_buffer_content)", size: length + 20, ms: 90 },
      { kind: "Text.sliceString", size: Math.ceil(0.9 * length), ms: 20 },
    ];
    expect(wholeDocumentCalls(calls, length)).toEqual(calls);
  });

  it("leaves out a part of the document and a kind the check does not hold", () => {
    const calls = [
      { kind: "Text.sliceString", size: Math.floor(0.5 * length), ms: 10 },
      { kind: "TextDecoder.decode", size: length, ms: 34 },
      { kind: "crypto.subtle.digest", size: length, ms: 60 },
    ];
    expect(wholeDocumentCalls(calls, length)).toEqual([]);
  });

  it("leaves out a whole document under 1 MiB", () => {
    expect(wholeDocumentCalls([{ kind: "Text.toString", size: 40_000, ms: 0 }], 40_000)).toEqual([]);
  });

  it("names the window, the call and its size", () => {
    expect(wholeDocumentProblems("the typing pause", [{ kind: "Text.toString", size: 52_428_800, ms: 27 }], 52_428_800)).toEqual([
      "the typing pause ran Text.toString of 52,428,800 characters",
    ]);
  });

  it("holds every scenario that opens as a large file, and only those", () => {
    for (const scenario of SCENARIOS) {
      expect(holdsWholeDocumentCalls(scenario), scenario.id).toBe(scenario.opensAs === "LargeFile");
    }
    for (const id of FIND_RATE.scenarios) {
      expect(holdsWholeDocumentCalls(SCENARIOS.find((scenario) => scenario.id === id)!), id).toBe(true);
    }
  });
});

describe("editor perf gate find rate", () => {
  const profile: CpuProfile = {
    nodes: [
      { id: 1, callFrame: { functionName: "(root)" } },
      { id: 2, callFrame: { functionName: "(idle)" } },
      { id: 3, callFrame: { functionName: "scanSlice" } },
      { id: 4, callFrame: { functionName: "(program)" } },
    ],
    samples: [2, 3, 3, 4, 2, 3],
    timeDeltas: [0, 5_000, 1_000, 2_000, 3_000, 10_000, 500],
  };

  it("counts every sample but the idle ones, each until the next", () => {
    // 3 -> 1 ms, 3 -> 2 ms, 4 -> 3 ms, 3 -> 0.5 ms; the two idle samples are left out.
    expect(busyMs(profile)).toBeCloseTo(6.5);
  });

  it("holds the slower word's median run to the budget", () => {
    const size = 50 * MiB;
    const runs = [
      { word: "zebrafish", busyMs: 60 },
      { word: "zebrafish", busyMs: 70 },
      { word: "zebrafish", busyMs: 200 },
      { word: "qxjzv", busyMs: 50 },
      { word: "qxjzv", busyMs: 55 },
      { word: "qxjzv", busyMs: 45 },
    ];
    const rate = findRate(runs, size);
    expect(rate.perWord).toEqual({ zebrafish: 70 / 50, qxjzv: 50 / 50 });
    expect(rate.msPerMiB).toBeCloseTo(1.4);
    expect(rate.problems).toEqual([]);
  });

  it("names a rate over the budget", () => {
    const rate = findRate([{ word: "zebrafish", busyMs: 221.5 }], 50 * MiB);
    expect(rate.problems).toEqual([`the find rate 4.43 ms/MiB is over ${FIND_RATE.budgetMsPerMiB} ms/MiB`]);
  });

  it("fails a rate it did not measure", () => {
    expect(findRate([], 50 * MiB).problems).toEqual(["the find rate was not measured"]);
  });
});

describe("editor perf gate paired typing", () => {
  it("takes the middle value, or the mean of the two middle values", () => {
    expect(middle([3, 1, 2])).toBe(2);
    expect(middle([4, 1, 3, 2])).toBe(2.5);
    expect(middle([])).toBeNull();
  });

  it("takes the noise from the median absolute deviation", () => {
    expect(noiseBand([8.7, 12.1, 12.2, 8, 9])).toBeCloseTo(1.4826);
    expect(noiseBand([])).toBeNull();
  });

  const stats = (p50: number, max: number) => ({ p50, max });

  it("fails typing that sits above the counts off by more than the band", () => {
    const offs = [9.0, 9.4, 8.8, 9.6, 9.2, 9.8];
    const rounds: TypingRound[] = offs.map((p50) => ({
      off: stats(p50, p50 + 10),
      on: stats(p50 + 3, p50 + 10.1),
      find: stats(p50 + 0.5, p50 + 10),
    }));
    const verdict = pairedTyping(rounds);
    expect(verdict.compared.on.p50.noise).toBeCloseTo(1.4826 * 0.3);
    expect(verdict.compared.on.p50.band).toBeCloseTo(PAIRED_TYPING.bandScale * 1.4826 * 0.3);
    expect(verdict.problems).toEqual([
      "typing with the counts on: the p50 sits 3.0 ms above the counts off, over the band of 0.9 ms",
    ]);
  });

  it("passes a difference of more than the noise and less than the band", () => {
    const rounds: TypingRound[] = [
      { off: stats(8.7, 21.7), on: stats(12, 21.5), find: stats(9.6, 20.8) },
      { off: stats(12.1, 24), on: stats(12, 20.9), find: stats(11.7, 18.7) },
      { off: stats(12.2, 21.7), on: stats(11.4, 22.3), find: stats(10.5, 19.8) },
      { off: stats(8, 17), on: stats(12, 22), find: stats(14, 18) },
      { off: stats(9, 21), on: stats(11, 18), find: stats(14, 18) },
    ];
    const { compared, problems } = pairedTyping(rounds);
    expect(compared.on.p50.delta).toBeCloseTo(2);
    expect(compared.on.p50.delta!).toBeGreaterThan(compared.on.p50.noise!);
    expect(problems).toEqual([]);
  });

  it("passes typing that differs from the counts off by less than their noise", () => {
    const offs = [9.1, 9.8, 8.6, 10.4, 9.3, 9.9];
    const rounds: TypingRound[] = offs.map((p50, index) => ({
      off: stats(p50, p50 + 10),
      on: stats(p50 + (index % 2 ? 0.2 : -0.2), p50 + 10.1),
      find: stats(p50 + (index % 2 ? -0.1 : 0.1), p50 + 9.9),
    }));
    expect(pairedTyping(rounds).problems).toEqual([]);
  });

  it("fails a round it did not measure", () => {
    const rounds: TypingRound[] = [
      { off: stats(9, 19), on: stats(9, 19), find: stats(9, 19) },
      { off: stats(9, 19), on: null, find: stats(9, 19) },
    ];
    expect(pairedTyping(rounds).problems).toEqual(["typing with the counts on was not measured in every round"]);
  });
});

describe("editor perf gate tab switch", () => {
  const kept: EditorSnapshot = { undoDepth: 1, anchor: 2_621_443, head: 2_621_452, topLine: 18_012, length: 5_138_022 };
  const checked = { set_buffer_view_position: 1, note_file_unchanged: 1 };

  it("passes a switch back that kept the editor, asked once whether the file changed and read nothing", () => {
    const bigCalls = [{ kind: "Text.sliceString", size: 65_536, ms: 0.1 }];
    expect(tabSwitchProblems({ before: kept, after: { ...kept }, ipcCalls: checked, bigCalls })).toEqual([]);
  });

  it("names a read of the file, a lost undo, a moved selection and a moved view", () => {
    const after = { undoDepth: 0, anchor: 0, head: 0, topLine: 1, length: kept.length };
    const ipcCalls = { ...checked, read_buffer_content: 1, note_disk_state: 1 };
    expect(tabSwitchProblems({ before: kept, after, ipcCalls, bigCalls: [] })).toEqual([
      "switching back read the file again (1 read_buffer_content)",
      "switching back hashed the file again (1 note_disk_state)",
      "the undo depth went from 1 to 0",
      "the selection went from 2621443-2621452 to 0-0",
      "the top visible line went from 18012 to 1",
    ]);
  });

  it("fails a switch back that did not ask whether the file changed, or asked twice", () => {
    expect(tabSwitchProblems({ before: kept, after: kept, ipcCalls: { set_buffer_view_position: 1 } })).toEqual([
      "switching back asked whether the file changed 0 times, not once",
    ]);
    expect(tabSwitchProblems({ before: kept, after: kept, ipcCalls: { note_file_unchanged: 2 } })).toEqual([
      "switching back asked whether the file changed 2 times, not once",
    ]);
  });

  it("fails a switch back that took the whole document, in any tier", () => {
    const bigCalls = [{ kind: "Text.toString", size: kept.length, ms: 15.2 }];
    expect(tabSwitchProblems({ before: kept, after: kept, ipcCalls: checked, bigCalls })).toEqual([
      "switching back ran Text.toString of 5,138,022 characters",
    ]);
  });

  it("fails a switch with nothing to undo before it", () => {
    const empty = { ...kept, undoDepth: 0 };
    expect(tabSwitchProblems({ before: empty, after: empty, ipcCalls: checked })).toEqual(["there was nothing to undo before the switch"]);
  });

  it("fails a switch with no editor to compare", () => {
    expect(tabSwitchProblems({ before: kept, after: null, ipcCalls: {} })).toEqual(["the editor was not there to compare"]);
  });
});

describe("editor perf gate section 6 checks", () => {
  const ids = (cell: string) => [...cell.matchAll(/`([^`]+)`/g)].map((match) => match[1]);

  it("runs each check on scenarios the gate has", () => {
    const known = new Set(SCENARIOS.map((scenario) => scenario.id));
    for (const id of [...FIND_RATE.scenarios, ...TAB_SWITCH.scenarios, PAIRED_TYPING.scenario]) expect(known.has(id), id).toBe(true);
    expect(PAIRED_TYPING.rounds).toBeGreaterThanOrEqual(5);
    expect(PAIRED_TYPING.rounds % 3, "each of the three conditions takes each place equally often").toBe(0);
  });

  it("states the same checks in docs/perf-budgets.md", () => {
    const rows = Object.fromEntries(tableRows(doc("docs/perf-budgets.md"), "editor-perf-section-6").map((row) => [row[0], row.slice(1)]));
    expect(Object.keys(rows)).toEqual(["Find rate", "Whole-document calls", "Tab switch back", "Paired typing"]);
    expect(ids(rows["Find rate"][0])).toEqual(FIND_RATE.scenarios);
    expect(rows["Find rate"][1]).toBe(`${FIND_RATE.budgetMsPerMiB} ms per MiB, the slower word's median of ${FIND_RATE.runs} runs`);
    expect(ids(rows["Tab switch back"][0])).toEqual(TAB_SWITCH.scenarios);
    expect(ids(rows["Paired typing"][0])).toEqual([PAIRED_TYPING.scenario]);
    expect(rows["Paired typing"][0]).toContain(`${PAIRED_TYPING.rounds} rounds`);
    expect(rows["Paired typing"][1]).toContain(`${PAIRED_TYPING.bandScale} times the noise`);
  });
});
