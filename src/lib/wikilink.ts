/**
 * The note name inside a `[[…]]` target.
 *
 * Mirrors `writ_core::notes::links::parse_wikilink`: the alias is what follows
 * the first `|`, the heading is what follows the first `#` before it, and a
 * folder prefix is everything up to the last `/`. What is left is the note's
 * name, which is what a surface shows and what `Create note` names a file.
 */
export function wikilinkName(target: string): string {
  const withoutAlias = target.split("|", 1)[0];
  const withoutHeading = withoutAlias.split("#", 1)[0];
  const parts = withoutHeading.split(/[\\/]/);
  return (parts[parts.length - 1] ?? "").trim();
}
