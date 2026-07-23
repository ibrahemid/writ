export interface SnippetSegment {
  text: string;
  matched: boolean;
}

export interface SearchHit {
  buffer_id: string;
  title: string;
  line: number | null;
  snippet: SnippetSegment[];
}

export interface SearchResults {
  hits: SearchHit[];
  total: number;
}

// Workspace search (ADR-026).

export interface FileHit {
  path: string;
  name: string;
  score: number;
}

export interface IndexStatus {
  file_count: number;
  truncated: boolean;
  has_workspace: boolean;
}

export interface ContentHit {
  path: string;
  line: number;
  snippet: SnippetSegment[];
}

export interface GrepOutcome {
  hit_count: number;
  files_scanned: number;
  truncated: boolean;
  cancelled: boolean;
}

export interface SearchBatch {
  generation: number;
  hits: ContentHit[];
  outcome: GrepOutcome | null;
}
