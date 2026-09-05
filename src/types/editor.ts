/** Which markdown constructs the caret currently sits inside. */
export interface ActiveFormats {
  bold: boolean;
  italic: boolean;
  code: boolean;
  bullet: boolean;
  task: boolean;
}

/** Nothing is under the caret: also the state of a buffer that is not prose. */
export const NO_ACTIVE_FORMATS: ActiveFormats = Object.freeze({
  bold: false,
  italic: false,
  code: false,
  bullet: false,
  task: false,
});
