export interface ContinueResult {
  nextText: string;
  nextPos: number;
}

export interface WrapResult {
  nextText: string;
  selStart: number;
  selEnd: number;
}

const ORDERED = /^(\s*)(\d+)\.(\s+)(.*)$/;
const TASK = /^(\s*)([-*+])\s+\[([ xX])\]\s(.*)$/;
const BULLET = /^(\s*)([-*+])\s+(.*)$/;
const QUOTE = /^(\s*)>\s?(.*)$/;

function lineBounds(text: string, pos: number): { start: number; end: number } {
  const start = text.lastIndexOf('\n', pos - 1) + 1;
  const nl = text.indexOf('\n', pos);
  return { start, end: nl < 0 ? text.length : nl };
}

// Enter inside a list/quote line: continue the marker, or terminate an empty item.
// Returns null to let the caller insert a plain newline.
export function continueListOnEnter(text: string, pos: number): ContinueResult | null {
  const { start } = lineBounds(text, pos);
  const line = text.slice(start, pos);

  const task = line.match(TASK);
  if (task) {
    const indent = task[1] ?? '';
    const glyph = task[2] ?? '-';
    if ((task[4] ?? '').trim() === '') return terminate(text, start, pos);
    return continueWith(text, pos, `${indent}${glyph} [ ] `);
  }

  const ordered = line.match(ORDERED);
  if (ordered) {
    const indent = ordered[1] ?? '';
    if ((ordered[4] ?? '').trim() === '') return terminate(text, start, pos);
    const next = Number(ordered[2] ?? '0') + 1;
    return continueOrdered(text, pos, `${indent}${next}. `, indent, next);
  }

  const bullet = line.match(BULLET);
  if (bullet) {
    const indent = bullet[1] ?? '';
    const glyph = bullet[2] ?? '-';
    if ((bullet[3] ?? '').trim() === '') return terminate(text, start, pos);
    return continueWith(text, pos, `${indent}${glyph} `);
  }

  const quote = line.match(QUOTE);
  if (quote) {
    const indent = quote[1] ?? '';
    if ((quote[2] ?? '').trim() === '') return terminate(text, start, pos);
    return continueWith(text, pos, `${indent}> `);
  }

  return null;
}

function continueWith(text: string, pos: number, marker: string): ContinueResult {
  const nextText = text.slice(0, pos) + '\n' + marker + text.slice(pos);
  return { nextText, nextPos: pos + 1 + marker.length };
}

function continueOrdered(
  text: string,
  pos: number,
  marker: string,
  indent: string,
  startNum: number,
): ContinueResult {
  const inserted = text.slice(0, pos) + '\n' + marker + text.slice(pos);
  const nextPos = pos + 1 + marker.length;

  const lines = inserted.split('\n');
  const insertedLine = inserted.slice(0, pos + 1).split('\n').length - 1;
  let counter = startNum;
  for (let i = insertedLine + 1; i < lines.length; i++) {
    const m = lines[i]!.match(ORDERED);
    if (!m || m[1] !== indent) break;
    counter += 1;
    lines[i] = `${m[1]}${counter}.${m[3]}${m[4]}`;
  }
  return { nextText: lines.join('\n'), nextPos };
}

// Remove the marker on an empty item, leaving a bare line where the caret was.
function terminate(text: string, start: number, pos: number): ContinueResult {
  const nextText = text.slice(0, start) + text.slice(pos);
  return { nextText, nextPos: start };
}

function unwrapResult(text: string, start: number, end: number, cut: number): WrapResult {
  return {
    nextText: text.slice(0, start - cut) + text.slice(start, end) + text.slice(end + cut),
    selStart: start - cut,
    selEnd: end - cut,
  };
}

// Toggle markdown markers around a selection. If the selection is already wrapped
// (markers inside or just outside it), unwrap; otherwise wrap.
export function wrapSelection(
  text: string,
  start: number,
  end: number,
  marker: string,
  closeMarker?: string,
): WrapResult {
  const close = closeMarker ?? marker;
  const selected = text.slice(start, end);

  if (
    selected.length >= marker.length + close.length &&
    selected.startsWith(marker) &&
    selected.endsWith(close)
  ) {
    const inner = selected.slice(marker.length, selected.length - close.length);
    return { nextText: text.slice(0, start) + inner + text.slice(end), selStart: start, selEnd: start + inner.length };
  }

  if (text.slice(start - marker.length, start) === marker && text.slice(end, end + close.length) === close) {
    return unwrapResult(text, start, end, marker.length);
  }

  const nextText = text.slice(0, start) + marker + selected + close + text.slice(end);
  return { nextText, selStart: start + marker.length, selEnd: end + marker.length };
}

const URL_ONLY = /^https?:\/\/\S+$/;

// Insert a markdown link. Selection becomes the label (caret in the url), a
// selected url becomes the target (caret in the label), empty inserts a scaffold.
export function insertLink(text: string, start: number, end: number): WrapResult {
  const selected = text.slice(start, end);

  if (selected && URL_ONLY.test(selected)) {
    const nextText = text.slice(0, start) + '[](' + selected + ')' + text.slice(end);
    return { nextText, selStart: start + 1, selEnd: start + 1 };
  }

  if (selected) {
    const scaffold = '[' + selected + '](https://)';
    const nextText = text.slice(0, start) + scaffold + text.slice(end);
    const caret = start + scaffold.length - 1;
    return { nextText, selStart: caret, selEnd: caret };
  }

  const nextText = text.slice(0, start) + '[]()' + text.slice(end);
  return { nextText, selStart: start + 1, selEnd: start + 1 };
}
