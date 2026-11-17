/** Why an image reference is not drawn in the editor. */
export type InlineImageFailure =
  | "outside_root"
  | "not_found"
  | "not_downloaded"
  | "too_large"
  | "not_an_image"
  | "unreachable"
  | "not_remote"
  | "local_address";

/**
 * Who started a web image load. An `automatic` one is refused for an address
 * on this computer or the local network, and the image keeps its Load image
 * button.
 */
export type RemoteImageLoad = "asked" | "automatic";

const FAILURES: readonly string[] = [
  "outside_root",
  "not_found",
  "not_downloaded",
  "too_large",
  "not_an_image",
  "unreachable",
  "not_remote",
  "local_address",
];

/** Whether a host-supplied identifier is one of the refusals. */
export function isInlineImageFailure(value: unknown): value is InlineImageFailure {
  return typeof value === "string" && FAILURES.includes(value);
}
