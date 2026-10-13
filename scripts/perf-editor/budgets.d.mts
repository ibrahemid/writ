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
  spec: CorpusSpec;
}

export type Measurements = Partial<Record<MetricKey, number | null>>;

export const MiB: number;
export const METRICS: readonly Metric[];
export const SCENARIOS: readonly Scenario[];
export const AUDIT_FIGURES: Readonly<Record<string, Partial<Record<MetricKey, number>>>>;
export const BUDGETS: Readonly<Record<string, Record<MetricKey, number>>>;
export const TOGGLE_BUDGETS: Readonly<{ coldP95Ms: number; warmP95Ms: number }>;

export function median(values: readonly number[]): number | null;
export function percentile(values: readonly number[], p: number): number | null;
export function maximum(values: readonly number[]): number | null;
export function overBudget(
  scenarioId: string,
  measured: Measurements,
  budgets?: Readonly<Record<string, Partial<Record<MetricKey, number>>>>,
): string[];
export function formatValue(value: number | null | undefined, unit: "ms" | "MiB"): string;
