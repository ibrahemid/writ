export interface SearchTerm {
  query: string;
  caseSensitive: boolean;
  wholeWord: boolean;
  regexp: boolean;
  replace: string;
}

export interface MatchState {
  current: number;
  total: number;
  capped: boolean;
  /** The document is still being read: `total` counts the matches found so far. */
  pending?: boolean;
  /** The query is a regular expression that does not compile. */
  invalid?: boolean;
  /** The regular expression engine gave up reading the document: a match attempt outgrew its stack. */
  failed?: boolean;
}

export interface MatchTick {
  fraction: number;
}

export interface EditorSearchController {
  setQuery(term: SearchTerm): void;
  next(): void;
  previous(): void;
  replaceCurrent(): void;
  replaceAll(): void;
  matchState(): MatchState;
  matchPositions(limit: number): MatchTick[];
  clear(): void;
}
