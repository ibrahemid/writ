import type { CorpusSpec } from "../../src/__tests__/editor/fixtures/editor-corpus";

export type MetricKey =
  | "firstPaintMs"
  | "heapMiB"
  | "jumpToEndMs"
  | "typingP50Ms"
  | "typingP95Ms"
  | "typingMaxMs"
  | "idleTaskMs"
  | "findKeyP50Ms"
  | "findKeyMaxMs"
  | "findSettleMs"
  | "findTaskMs"
  | "findTypingP50Ms"
  | "findTypingMaxMs"
  | "findTypingTaskMs";

export interface Metric {
  key: MetricKey;
  label: string;
  unit: "ms" | "MiB";
  /** No audit floor: the audit measured the count this gate replaced. */
  find?: true;
}

export type Tier = "Normal" | "LargeFile" | "LargeFileConfirm";
export type OpensAs = "Normal" | "LongLines" | "LargeFile";

export interface Scenario {
  id: string;
  tier: Tier;
  opensAs: OpensAs;
  large?: true;
  /** The status bar shows the word, character and token counts. */
  counts?: true;
  /** Added after the editor-perf audit, which measured no such file: no audit floor. */
  afterAudit?: true;
  /** The file shows in two panes, and the typing goes to the second (ADR-056). */
  split?: true;
  /** Selects all, then extends a selection a row at a time, and checks where the selection paints. */
  selection?: true;
  spec: CorpusSpec;
}

export type Measurements = Partial<Record<MetricKey, number | null>>;

export interface LongTask {
  /** `PerformanceEntry.startTime`, on the page's clock. */
  start: number;
  duration: number;
}

export const MiB: number;
export const METRICS: readonly Metric[];
export const SCENARIOS: readonly Scenario[];
export const AUDIT_FIGURES: Readonly<Record<string, Partial<Record<MetricKey, number>>>>;
export const BUDGETS: Readonly<Record<string, Record<MetricKey, number>>>;
export const TOGGLE_BUDGETS: Readonly<{ coldP95Ms: number; warmP95Ms: number }>;

export function median(values: readonly number[]): number | null;
export function percentile(values: readonly number[], p: number): number | null;
export function maximum(values: readonly number[]): number | null;
/** The tasks that started at or after `from` and before `to`, in their order. */
export function filterTasksStartedIn(tasks: readonly LongTask[], from: number, to: number): LongTask[];
export function overBudget(
  scenarioId: string,
  measured: Measurements,
  budgets?: Readonly<Record<string, Partial<Record<MetricKey, number>>>>,
): string[];
export function formatValue(value: number | null | undefined, unit: "ms" | "MiB"): string;

export const FIND_RATE: Readonly<{ scenarios: readonly string[]; runs: number; budgetMsPerMiB: number }>;
export const TAB_SWITCH: Readonly<{ scenarios: readonly string[]; other: CorpusSpec }>;
export const PAIRED_TYPING: Readonly<{ scenario: string; rounds: number; characters: number; bandScale: number }>;
export const WHOLE_DOCUMENT: Readonly<{ kinds: readonly string[]; minChars: number; share: number }>;

/** A call the harness saw take a value of `size` characters. */
export interface BigCall {
  kind: string;
  size: number;
  ms: number;
}

export function holdsWholeDocumentCalls(scenario: Scenario): boolean;
export function wholeDocumentCalls(calls: readonly BigCall[], docLength: number): BigCall[];
export function wholeDocumentProblems(where: string, calls: readonly BigCall[], docLength: number): string[];

/** The parts of a CDP `Profiler.Profile` the find rate reads. */
export interface CpuProfile {
  nodes: readonly { id: number; callFrame: { functionName: string } }[];
  samples: readonly number[];
  timeDeltas: readonly number[];
}
export function busyMs(profile: CpuProfile): number;
export interface FindRun {
  word: string;
  busyMs: number;
}
export function findRate(
  runs: readonly FindRun[],
  sizeBytes: number,
): { perWord: Record<string, number | null>; msPerMiB: number | null; problems: string[] };

export function middle(values: readonly number[]): number | null;
export function noiseBand(values: readonly number[]): number | null;

export interface TypingStats {
  p50: number | null;
  max: number | null;
}
export interface TypingRound {
  off: TypingStats | null;
  on: TypingStats | null;
  find: TypingStats | null;
}
export interface Comparison {
  /** The median of the round-by-round differences from the counts off. */
  delta: number | null;
  /** The counts-off runs' noise, from `noiseBand`. */
  noise: number | null;
  /** `PAIRED_TYPING.bandScale` times the noise: the most `delta` may be. */
  band: number | null;
  rounds: number;
}
export function pairedTyping(rounds: readonly TypingRound[]): {
  compared: Record<"on" | "find", Record<"p50" | "max", Comparison>>;
  problems: string[];
};

export interface EditorSnapshot {
  undoDepth: number;
  head: number;
  anchor: number;
  topLine: number;
  /** The document's length in UTF-16 code units. */
  length: number;
}
export function tabSwitchProblems(trip: {
  before: EditorSnapshot | null;
  after: EditorSnapshot | null;
  ipcCalls: Record<string, number>;
  bigCalls?: readonly BigCall[];
}): string[];
