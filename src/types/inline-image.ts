/** Why an image reference is not drawn in the editor. */
export type InlineImageFailure =
  | "outside_root"
  | "not_found"
  | "not_downloaded"
  | "too_large"
  | "not_an_image"
  | "unreachable"
  | "not_remote"
  | "local_address"
  | "resolves_to_this_computer"
  | "resolves_to_local_network"
  | "redirects_to_this_computer"
  | "redirects_to_local_network"
  | "not_secure";

/** Where a local address leads, as the host classified it. */
export type LocalScope = "this_computer" | "local_network";

/**
 * Who started a web image load. An `automatic` one is refused for a local
 * host or a name that resolves to a local address, and the image keeps its
 * Load image button. Either is refused for a redirect to a local address at
 * another host.
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
  "resolves_to_this_computer",
  "resolves_to_local_network",
  "redirects_to_this_computer",
  "redirects_to_local_network",
  "not_secure",
];

/** Whether a host-supplied identifier is one of the refusals. */
export function isInlineImageFailure(value: unknown): value is InlineImageFailure {
  return typeof value === "string" && FAILURES.includes(value);
}
