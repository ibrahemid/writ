import { SPRITE } from "./sprite.generated";

/**
 * The `<symbol>` definitions every `<Icon>` references. Mounted once, at the
 * top of the app tree: a `<use href="#ph-…">` resolves against the document,
 * so the sprite has to be in the DOM before any icon paints.
 */
export default function IconSprite() {
  return (
    <svg
      aria-hidden="true"
      style={{ position: "absolute", width: "0", height: "0", overflow: "hidden" }}
      innerHTML={SPRITE}
    />
  );
}
