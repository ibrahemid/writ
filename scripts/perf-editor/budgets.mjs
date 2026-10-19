export const MiB = 1024 * 1024;

export const METRICS = [
  { key: "firstPaintMs", label: "First paint", unit: "ms" },
  { key: "heapMiB", label: "JS heap growth", unit: "MiB" },
  { key: "jumpToEndMs", label: "Jump to end", unit: "ms" },
  { key: "typingP50Ms", label: "Typing p50", unit: "ms" },
  { key: "typingP95Ms", label: "Typing p95", unit: "ms" },
  { key: "typingMaxMs", label: "Typing max", unit: "ms" },
  { key: "idleTaskMs", label: "Longest task after typing", unit: "ms" },
  { key: "findKeyP50Ms", label: "Find keystroke p50", unit: "ms", find: true },
  { key: "findKeyMaxMs", label: "Find keystroke max", unit: "ms", find: true },
  { key: "findSettleMs", label: "Find to final count", unit: "ms", find: true },
  { key: "findTaskMs", label: "Longest task typing in Find", unit: "ms", find: true },
  { key: "findTypingP50Ms", label: "Typing with Find open p50", unit: "ms", find: true },
  { key: "findTypingMaxMs", label: "Typing with Find open max", unit: "ms", find: true },
  { key: "findTypingTaskMs", label: "Longest task typing with Find open", unit: "ms", find: true },
];

export const SCENARIOS = [
  {
    id: "prose-1MiB",
    tier: "Normal",
    opensAs: "Normal",
    spec: { name: "prose-1MiB.txt", kind: "prose", size: 1 * MiB },
  },
  {
    id: "prose-4.9MiB",
    tier: "Normal",
    opensAs: "Normal",
    spec: { name: "prose-4.9MiB.txt", kind: "prose", size: Math.round(4.9 * MiB) },
  },
  {
    id: "prose-4.9MiB-counts",
    tier: "Normal",
    opensAs: "Normal",
    counts: true,
    spec: { name: "prose-4.9MiB.txt", kind: "prose", size: Math.round(4.9 * MiB) },
  },
  {
    id: "md-4.9MiB",
    tier: "Normal",
    opensAs: "Normal",
    spec: { name: "notes-4.9MiB.md", kind: "markdown", size: Math.round(4.9 * MiB), sectionBytes: 10_480 },
  },
  {
    id: "md-blocks",
    tier: "Normal",
    opensAs: "Normal",
    afterAudit: true,
    selection: true,
    spec: { name: "blocks-50-tables-20-diagrams.md", kind: "markdown-blocks", size: 0, tables: 50, diagrams: 20 },
  },
  {
    id: "prose-1MiB-split",
    tier: "Normal",
    opensAs: "Normal",
    afterAudit: true,
    split: true,
    selection: true,
    spec: { name: "prose-1MiB.txt", kind: "prose", size: 1 * MiB },
  },
  {
    id: "json-1line-5MiB",
    tier: "Normal",
    opensAs: "LongLines",
    selection: true,
    spec: { name: "single-line-5MiB.json", kind: "single-line-json", size: 5 * MiB },
  },
  {
    id: "prose-5.1MiB",
    tier: "LargeFile",
    opensAs: "LargeFile",
    spec: { name: "prose-5.1MiB.txt", kind: "prose", size: Math.round(5.1 * MiB) },
  },
  {
    id: "log-200k",
    tier: "LargeFile",
    opensAs: "LargeFile",
    spec: { name: "server-200k.log", kind: "log", size: 0, lines: 200_000 },
  },
  {
    id: "log-450k",
    tier: "LargeFile",
    opensAs: "LargeFile",
    spec: { name: "server-450k.log", kind: "log", size: 0, lines: 450_000 },
  },
  {
    id: "prose-50MiB",
    tier: "LargeFile",
    opensAs: "LargeFile",
    spec: { name: "prose-50MiB.txt", kind: "prose", size: 50 * MiB },
  },
  {
    id: "prose-60MiB",
    tier: "LargeFileConfirm",
    opensAs: "LargeFile",
    spec: { name: "prose-60MiB.txt", kind: "prose", size: 60 * MiB },
  },
  {
    id: "prose-300MiB",
    tier: "LargeFileConfirm",
    opensAs: "LargeFile",
    large: true,
    spec: { name: "prose-300MiB.txt", kind: "prose", size: 300 * MiB },
  },
  {
    id: "log-300MiB-counts",
    tier: "LargeFileConfirm",
    opensAs: "LargeFile",
    large: true,
    counts: true,
    spec: { name: "server-300MiB.log", kind: "log", size: 0, lines: 2_800_000 },
  },
];

// The editor-perf audit's figures (2026-10-03, Chrome for Testing 153, Apple
// M1 Pro); log-450k and prose-60MiB take the 50 MiB row. With the counts on,
// the audit typed at p50 72 ms in the 4.9 MiB file and crashed the renderer
// opening 300 MiB, so prose-4.9MiB-counts takes the counts-off 4.9 MiB row and
// log-300MiB-counts the 300 MiB row.
export const AUDIT_FIGURES = {
  "prose-1MiB": { firstPaintMs: 30, heapMiB: 1.7, jumpToEndMs: 9, typingP50Ms: 12.2, typingP95Ms: 19.6, typingMaxMs: 20.2, idleTaskMs: 0, findKeyP50Ms: 124, findTypingP50Ms: 121 },
  "prose-4.9MiB": { firstPaintMs: 57, heapMiB: 7.0, jumpToEndMs: 13, typingP50Ms: 8.9, typingP95Ms: 17.6, typingMaxMs: 18.4, idleTaskMs: 0, findKeyP50Ms: 547, findTypingP50Ms: 558 },
  "prose-4.9MiB-counts": { firstPaintMs: 57, heapMiB: 7.0, jumpToEndMs: 13, typingP50Ms: 8.9, typingP95Ms: 17.6, typingMaxMs: 18.4, idleTaskMs: 0, findKeyP50Ms: 583, findTypingP50Ms: 614 },
  "md-4.9MiB": { firstPaintMs: 66, heapMiB: 9.1, jumpToEndMs: 19, typingP50Ms: 31.5, typingP95Ms: 49.5, typingMaxMs: 71.2, idleTaskMs: 0, findKeyP50Ms: 555, findTypingP50Ms: 570 },
  "json-1line-5MiB": { firstPaintMs: 83, heapMiB: 0.3, jumpToEndMs: 6, typingP50Ms: 20.7, typingP95Ms: 29.9, typingMaxMs: 30.0, idleTaskMs: 0, findKeyP50Ms: 542, findTypingP50Ms: 24 },
  "prose-5.1MiB": { firstPaintMs: 49, heapMiB: 7.2, jumpToEndMs: 20, typingP50Ms: 10.9, typingP95Ms: 18.3, typingMaxMs: 20.9, idleTaskMs: 0, findKeyP50Ms: 570, findTypingP50Ms: 14 },
  "log-200k": { firstPaintMs: 87, heapMiB: 27, jumpToEndMs: 10, typingP50Ms: 11.9, typingP95Ms: 17.2, typingMaxMs: 19.7, idleTaskMs: 53, findKeyP50Ms: 2384, findTypingP50Ms: 14 },
  "log-450k": { firstPaintMs: 330, heapMiB: 67, jumpToEndMs: 20, typingP50Ms: 12.4, typingP95Ms: 17.6, typingMaxMs: 17.7, idleTaskMs: 84, findKeyP50Ms: 6061, findTypingP50Ms: 7 },
  "prose-50MiB": { firstPaintMs: 330, heapMiB: 67, jumpToEndMs: 20, typingP50Ms: 12.4, typingP95Ms: 17.6, typingMaxMs: 17.7, idleTaskMs: 84, findKeyP50Ms: 6061, findTypingP50Ms: 7 },
  "prose-60MiB": { firstPaintMs: 330, heapMiB: 67, jumpToEndMs: 20, typingP50Ms: 12.4, typingP95Ms: 17.6, typingMaxMs: 17.7, idleTaskMs: 84, findKeyP50Ms: 6061, findTypingP50Ms: 7 },
  "prose-300MiB": { firstPaintMs: 2187, heapMiB: 403, jumpToEndMs: 20, typingP50Ms: 9.9, typingP95Ms: 17.8, typingMaxMs: 17.8, idleTaskMs: 736, findKeyP50Ms: 34673, findTypingP50Ms: 16 },
  "log-300MiB-counts": { firstPaintMs: 2187, heapMiB: 403, jumpToEndMs: 20, typingP50Ms: 9.9, typingP95Ms: 17.8, typingMaxMs: 17.8, idleTaskMs: 736, findKeyP50Ms: 34673, findTypingP50Ms: 16 },
};

export const BUDGETS = {
  "prose-1MiB": { firstPaintMs: 80, heapMiB: 4, jumpToEndMs: 65, typingP50Ms: 40, typingP95Ms: 60, typingMaxMs: 100, idleTaskMs: 100, findKeyP50Ms: 35, findKeyMaxMs: 100, findSettleMs: 475, findTaskMs: 100, findTypingP50Ms: 50, findTypingMaxMs: 100, findTypingTaskMs: 100 },
  "prose-4.9MiB": { firstPaintMs: 145, heapMiB: 11, jumpToEndMs: 75, typingP50Ms: 35, typingP95Ms: 55, typingMaxMs: 100, idleTaskMs: 100, findKeyP50Ms: 35, findKeyMaxMs: 100, findSettleMs: 465, findTaskMs: 100, findTypingP50Ms: 40, findTypingMaxMs: 100, findTypingTaskMs: 100 },
  "prose-4.9MiB-counts": { firstPaintMs: 145, heapMiB: 11, jumpToEndMs: 75, typingP50Ms: 35, typingP95Ms: 55, typingMaxMs: 100, idleTaskMs: 100, findKeyP50Ms: 35, findKeyMaxMs: 100, findSettleMs: 465, findTaskMs: 100, findTypingP50Ms: 40, findTypingMaxMs: 100, findTypingTaskMs: 100 },
  "md-4.9MiB": { firstPaintMs: 235, heapMiB: 15, jumpToEndMs: 70, typingP50Ms: 100, typingP95Ms: 160, typingMaxMs: 305, idleTaskMs: 305, findKeyP50Ms: 35, findKeyMaxMs: 100, findSettleMs: 465, findTaskMs: 100, findTypingP50Ms: 110, findTypingMaxMs: 305, findTypingTaskMs: 305 },
  "md-blocks": { firstPaintMs: 95, heapMiB: 6, jumpToEndMs: 165, typingP50Ms: 35, typingP95Ms: 55, typingMaxMs: 100, idleTaskMs: 100, findKeyP50Ms: 35, findKeyMaxMs: 100, findSettleMs: 510, findTaskMs: 100, findTypingP50Ms: 45, findTypingMaxMs: 100, findTypingTaskMs: 100 },
  "prose-1MiB-split": { firstPaintMs: 80, heapMiB: 4, jumpToEndMs: 55, typingP50Ms: 40, typingP95Ms: 60, typingMaxMs: 100, idleTaskMs: 100, findKeyP50Ms: 35, findKeyMaxMs: 100, findSettleMs: 485, findTaskMs: 100, findTypingP50Ms: 45, findTypingMaxMs: 100, findTypingTaskMs: 100 },
  "json-1line-5MiB": { firstPaintMs: 260, heapMiB: 4, jumpToEndMs: 60, typingP50Ms: 70, typingP95Ms: 85, typingMaxMs: 110, idleTaskMs: 100, findKeyP50Ms: 35, findKeyMaxMs: 100, findSettleMs: 465, findTaskMs: 100, findTypingP50Ms: 120, findTypingMaxMs: 150, findTypingTaskMs: 100 },
  "prose-5.1MiB": { firstPaintMs: 120, heapMiB: 11, jumpToEndMs: 55, typingP50Ms: 35, typingP95Ms: 55, typingMaxMs: 100, idleTaskMs: 100, findKeyP50Ms: 35, findKeyMaxMs: 100, findSettleMs: 465, findTaskMs: 100, findTypingP50Ms: 55, findTypingMaxMs: 100, findTypingTaskMs: 100 },
  "log-200k": { firstPaintMs: 305, heapMiB: 41, jumpToEndMs: 55, typingP50Ms: 40, typingP95Ms: 60, typingMaxMs: 210, idleTaskMs: 210, findKeyP50Ms: 35, findKeyMaxMs: 100, findSettleMs: 465, findTaskMs: 100, findTypingP50Ms: 45, findTypingMaxMs: 240, findTypingTaskMs: 240 },
  "log-450k": { firstPaintMs: 655, heapMiB: 91, jumpToEndMs: 60, typingP50Ms: 40, typingP95Ms: 65, typingMaxMs: 490, idleTaskMs: 490, findKeyP50Ms: 40, findKeyMaxMs: 100, findSettleMs: 635, findTaskMs: 100, findTypingP50Ms: 45, findTypingMaxMs: 580, findTypingTaskMs: 580 },
  "prose-50MiB": { firstPaintMs: 980, heapMiB: 101, jumpToEndMs: 70, typingP50Ms: 35, typingP95Ms: 60, typingMaxMs: 530, idleTaskMs: 530, findKeyP50Ms: 40, findKeyMaxMs: 100, findSettleMs: 950, findTaskMs: 100, findTypingP50Ms: 35, findTypingMaxMs: 630, findTypingTaskMs: 630 },
  "prose-60MiB": { firstPaintMs: 1195, heapMiB: 121, jumpToEndMs: 60, typingP50Ms: 35, typingP95Ms: 55, typingMaxMs: 800, idleTaskMs: 800, findKeyP50Ms: 45, findKeyMaxMs: 100, findSettleMs: 1150, findTaskMs: 100, findTypingP50Ms: 45, findTypingMaxMs: 1170, findTypingTaskMs: 1155 },
  "prose-300MiB": { firstPaintMs: 6515, heapMiB: 605, jumpToEndMs: 75, typingP50Ms: 35, typingP95Ms: 60, typingMaxMs: 4625, idleTaskMs: 4625, findKeyP50Ms: 40, findKeyMaxMs: 100, findSettleMs: 4345, findTaskMs: 100, findTypingP50Ms: 40, findTypingMaxMs: 4625, findTypingTaskMs: 4625 },
  "log-300MiB-counts": { firstPaintMs: 2190, heapMiB: 403, jumpToEndMs: 70, typingP50Ms: 40, typingP95Ms: 60, typingMaxMs: 2335, idleTaskMs: 2335, findKeyP50Ms: 45, findKeyMaxMs: 75, findSettleMs: 2485, findTaskMs: 100, findTypingP50Ms: 40, findTypingMaxMs: 2690, findTypingTaskMs: 2690 },
};

export const TOGGLE_BUDGETS = { coldP95Ms: 250, warmP95Ms: 80 };

// Held to the editor-perf audit's section 6 targets; docs/perf-budgets.md
// states them. The timing checks run only with --strict, on the machine the
// targets were measured on.
export const FIND_RATE = {
  scenarios: ["prose-50MiB", "prose-300MiB", "log-300MiB-counts"],
  runs: 3,
  budgetMsPerMiB: 1.6,
};

export const TAB_SWITCH = {
  scenarios: ["prose-4.9MiB", "md-4.9MiB", "prose-50MiB"],
  other: { name: "other.txt", kind: "prose", size: 16 * 1024 },
};

export const PAIRED_TYPING = {
  scenario: "prose-4.9MiB",
  rounds: 12,
  characters: 120,
  bandScale: 2,
};

export const WHOLE_DOCUMENT = {
  kinds: ["Text.toString", "Text.sliceString", "TextEncoder.encode", "TextEncoder.encodeInto", "JSON.stringify"],
  minChars: MiB,
  share: 0.9,
};

/** The scenarios whose open and typing pauses may not turn the whole document into one value. */
export function holdsWholeDocumentCalls(scenario) {
  return scenario.opensAs === "LargeFile";
}

/** The calls in `calls` that took the whole of a document `docLength` characters long. */
export function wholeDocumentCalls(calls, docLength) {
  const least = Math.max(WHOLE_DOCUMENT.minChars, WHOLE_DOCUMENT.share * docLength);
  return calls.filter((call) => WHOLE_DOCUMENT.kinds.some((kind) => call.kind.startsWith(kind)) && call.size >= least);
}

export function wholeDocumentProblems(where, calls, docLength) {
  return wholeDocumentCalls(calls, docLength).map(
    (call) => `${where} ran ${call.kind} of ${call.size.toLocaleString("en-US")} characters`,
  );
}

/** Main-thread time a CPU profile spent on anything but idling, in ms. */
export function busyMs(profile) {
  const idle = new Set(profile.nodes.filter((node) => node.callFrame.functionName === "(idle)").map((node) => node.id));
  let busy = 0;
  for (let i = 0; i < profile.samples.length; i++) {
    if (!idle.has(profile.samples[i])) busy += (profile.timeDeltas[i + 1] ?? 0) / 1000;
  }
  return busy;
}

/**
 * The find rate of each word, the median of its runs in ms per MiB, and the
 * slower word's rate held to the budget.
 */
export function findRate(runs, sizeBytes) {
  const words = [...new Set(runs.map((run) => run.word))];
  const perWord = Object.fromEntries(
    words.map((word) => [word, middle(runs.filter((run) => run.word === word).map((run) => run.busyMs / (sizeBytes / MiB)))]),
  );
  const rates = Object.values(perWord);
  const msPerMiB = rates.length ? Math.max(...rates) : null;
  const problems =
    msPerMiB === null
      ? ["the find rate was not measured"]
      : msPerMiB > FIND_RATE.budgetMsPerMiB
        ? [`the find rate ${msPerMiB.toFixed(2)} ms/MiB is over ${FIND_RATE.budgetMsPerMiB} ms/MiB`]
        : [];
  return { perWord, msPerMiB, problems };
}

/** The middle value, or the mean of the two middle values. */
export function middle(values) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const half = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[half] : (sorted[half - 1] + sorted[half]) / 2;
}

/** The run-to-run spread of `values`: 1.4826 times their median absolute deviation, a standard deviation for normal noise. */
export function noiseBand(values) {
  const centre = middle(values);
  if (centre === null) return null;
  return 1.4826 * middle(values.map((value) => Math.abs(value - centre)));
}

/**
 * Typing with the counts on and with Find open against typing with the
 * counts off, round by round: the median of each round's difference, for the
 * runs' p50 and max, has to sit within PAIRED_TYPING.bandScale times the
 * noise of the counts-off runs.
 */
export function pairedTyping(rounds) {
  const problems = [];
  const compared = {};
  for (const [variant, label] of [
    ["on", "with the counts on"],
    ["find", "with Find open"],
  ]) {
    compared[variant] = {};
    for (const stat of ["p50", "max"]) {
      const usable = rounds.filter((round) => typeof round.off?.[stat] === "number" && typeof round[variant]?.[stat] === "number");
      const delta = middle(usable.map((round) => round[variant][stat] - round.off[stat]));
      const noise = noiseBand(usable.map((round) => round.off[stat]));
      const band = noise === null ? null : PAIRED_TYPING.bandScale * noise;
      compared[variant][stat] = { delta, noise, band, rounds: usable.length };
      if (usable.length < rounds.length || delta === null || band === null) {
        problems.push(`typing ${label} was not measured in every round`);
      } else if (delta > band) {
        problems.push(
          `typing ${label}: the ${stat} sits ${delta.toFixed(1)} ms above the counts off, over the band of ${band.toFixed(1)} ms`,
        );
      }
    }
  }
  return { compared, problems: [...new Set(problems)] };
}

/** What a switch to another tab and back changed in the editor and asked the host. */
export function tabSwitchProblems({ before, after, ipcCalls, bigCalls }) {
  if (!before || !after) return ["the editor was not there to compare"];
  const problems = [];
  if (before.undoDepth === 0) problems.push("there was nothing to undo before the switch");
  if ((ipcCalls.read_buffer_content ?? 0) > 0) {
    problems.push(`switching back read the file again (${ipcCalls.read_buffer_content} read_buffer_content)`);
  }
  if ((ipcCalls.note_disk_state ?? 0) > 0) {
    problems.push(`switching back hashed the file again (${ipcCalls.note_disk_state} note_disk_state)`);
  }
  const asked = ipcCalls.note_file_unchanged ?? 0;
  if (asked !== 1) problems.push(`switching back asked whether the file changed ${asked} times, not once`);
  problems.push(...wholeDocumentProblems("switching back", bigCalls ?? [], before.length));
  if (after.undoDepth !== before.undoDepth) problems.push(`the undo depth went from ${before.undoDepth} to ${after.undoDepth}`);
  if (after.anchor !== before.anchor || after.head !== before.head) {
    problems.push(`the selection went from ${before.anchor}-${before.head} to ${after.anchor}-${after.head}`);
  }
  if (after.topLine !== before.topLine) problems.push(`the top visible line went from ${before.topLine} to ${after.topLine}`);
  return problems;
}

export function median(values) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

// Nearest rank, as the audit's probe reported it.
export function percentile(values, p) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor((sorted.length * p) / 100))];
}

export function maximum(values) {
  return values.length === 0 ? null : Math.max(...values);
}

// The browser reports a long task after it ends, so one that began before a
// window was reset can arrive inside it; a window counts only the tasks that
// started in it.
export function filterTasksStartedIn(tasks, from, to) {
  return tasks.filter((task) => task.start >= from && task.start < to);
}

export function overBudget(scenarioId, measured, budgets = BUDGETS) {
  const budget = budgets[scenarioId];
  if (!budget) return [`${scenarioId} has no budget`];
  const failures = [];
  for (const metric of METRICS) {
    const value = measured[metric.key];
    const limit = budget[metric.key];
    if (typeof limit !== "number") {
      failures.push(`${metric.label} has no budget`);
    } else if (typeof value !== "number" || Number.isNaN(value)) {
      failures.push(`${metric.label} was not measured`);
    } else if (value > limit) {
      failures.push(`${metric.label} ${formatValue(value, metric.unit)} is over ${formatValue(limit, metric.unit)}`);
    }
  }
  return failures;
}

export function formatValue(value, unit) {
  if (typeof value !== "number" || Number.isNaN(value)) return "n/a";
  return unit === "MiB" ? `${value.toFixed(1)} MiB` : `${Math.round(value)} ms`;
}
