export type TransformCategory = "whitespace" | "punctuation" | "indentation" | "other";

export interface TransformMetadata {
  label: string;
  description: string;
  category: TransformCategory;
}

export interface TransformDescriptor {
  id: string;
  metadata: TransformMetadata;
}
