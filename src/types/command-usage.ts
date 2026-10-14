/** How often a palette command ran and when it last did (ADR-048). */
export interface CommandUsage {
  count: number;
  last_used_ms: number;
}

/** Runs of one command to add to what writ.db holds. */
export interface CommandUse {
  id: string;
  count: number;
  last_used_ms: number;
}
