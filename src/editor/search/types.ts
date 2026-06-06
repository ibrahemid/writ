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
