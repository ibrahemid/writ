/**
 * Where a note's YAML frontmatter block ends.
 *
 * The rule is `writ_core::notes::links::split_frontmatter`, which
 * `writ_render::split_frontmatter` and the prompt stripper already share: the
 * first line must be exactly `---` after trailing whitespace is trimmed, and a
 * later line must be exactly `---`. An unterminated block is body text, so a
 * note opening with a horizontal rule is not swallowed.
 *
 * The answer is a character offset: everything before it is frontmatter,
 * everything from it on is body. Zero means the text carries none.
 */
export function frontmatterEnd(text: string): number {
  const lines = splitInclusive(text);
  if (lines.length === 0 || !isFrontmatterFence(lines[0])) return 0;
  let offset = lines[0].length;
  for (let index = 1; index < lines.length; index++) {
    offset += lines[index].length;
    if (isFrontmatterFence(lines[index])) return offset;
  }
  return 0;
}

/**
 * Whether a line is a `---` frontmatter delimiter.
 *
 * The line ending is trimmed with the rest of the trailing whitespace, which
 * is what `str::trim_end` does on the Rust side.
 */
export function isFrontmatterFence(line: string): boolean {
  return line.trimEnd() === "---";
}

function splitInclusive(text: string): string[] {
  const out: string[] = [];
  let start = 0;
  for (let index = 0; index < text.length; index++) {
    if (text[index] === "\n") {
      out.push(text.slice(start, index + 1));
      start = index + 1;
    }
  }
  if (start < text.length) out.push(text.slice(start));
  return out;
}
