/**
 * True for a keystroke an input method is still composing with. WebKit sends
 * the Enter that confirms a Japanese, Chinese or Korean conversion with
 * `isComposing` false and keyCode 229, so both are read.
 */
export function isImeComposing(event: Pick<KeyboardEvent, "isComposing" | "keyCode">): boolean {
  return event.isComposing === true || event.keyCode === 229;
}
