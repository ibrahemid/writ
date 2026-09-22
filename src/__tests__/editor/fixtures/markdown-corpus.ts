// A deterministic markdown document holding every node the decoration
// builder branches on, so a change in the measured cost is a change in the
// builder rather than in the fixture.

const BLOCK = `# Heading one

Prose with **bold**, *italic*, ~~struck~~ and \`inline code\` in one sentence.

## Heading two

A [labelled link](https://example.com/docs) and a bare https://example.com/plain
address, plus an image: ![Alt text](shots/one.png)

> A quoted line.
> A second quoted line.

- First bullet
- Second bullet
  - Nested bullet

1. First numbered
2. Second numbered

- [ ] An open task
- [x] A completed task

### Heading three

\`\`\`ts
const value: number = 1;
function name(argument: string): string {
  return argument;
}
\`\`\`

| Column | Other |
|---|---|
| one | two |
| three | four |

---

Closing prose paragraph for the block.
`;

/** A deterministic markdown document of at least `bytes` characters. */
export function buildMarkdownCorpus(bytes: number): string {
  const parts: string[] = [];
  let length = 0;
  for (let index = 0; length < bytes; index++) {
    // The index keeps every repetition distinct, so no two blocks parse to
    // the same positions and an incremental parse cannot short-circuit.
    const part = `${BLOCK}\nSection ${index} ends here.\n\n`;
    parts.push(part);
    length += part.length;
  }
  return parts.join("");
}
