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
