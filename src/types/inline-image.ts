/** Why an image reference is not carried into the editor. */
export type InlineImageFailure = "outside_root" | "not_found" | "too_large" | "not_an_image";

const FAILURES: readonly string[] = ["outside_root", "not_found", "too_large", "not_an_image"];

/** Whether a host-supplied identifier is one of the refusals. */
export function isInlineImageFailure(value: unknown): value is InlineImageFailure {
  return typeof value === "string" && FAILURES.includes(value);
}
