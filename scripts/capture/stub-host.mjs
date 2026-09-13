// A chat host on localhost for the chat capture. It answers every
// chat-completions request with the same short reply and one proposal for
// the note the pane attached, so nothing leaves the machine and the frame is
// the same on every run.
//
//   node scripts/capture/stub-host.mjs [port]     (default 8791)
import { createServer } from 'node:http';

const port = Number(process.argv[2] ?? 8791);

// The pane sends attached notes as <note path="...">text</note>; the reply
// proposes a change to the first one.
function attachedNote(body) {
  const text = body.messages?.map((m) => (typeof m.content === 'string' ? m.content : '')).join('\n') ?? '';
  const match = /<note path="([^"]+)">([\s\S]*?)<\/note>/.exec(text);
  return match ? { path: match[1], text: match[2] } : null;
}

const NEW_OPENING =
  'The allotment in September is mostly about clearing. The courgettes have given up, the beans have gone stringy, and the garlic is not in yet. It is the month for compost and for planning, which is another way of saying it is the month for standing in the shed with a cup of tea.';

const BIRTHDAY = `---
tags: [home]
for: October
---
# Birthday ideas

## Small

- The olive oil from the market, the tin with the blue label.
- A proper bread knife. The one we have saws.

## Bigger

- Tickets for the spring tour, if they go on sale in time.
- A weekend somewhere with a long walk and a good pub. Not Lisbon, that is ours.

Or bake the [[Lemon olive oil cake]] and call it done.
`;

function revise(text) {
  if (text.includes('# Birthday ideas')) return BIRTHDAY;
  const start = text.indexOf('## Opening');
  const end = text.indexOf('## The recipe');
  if (start === -1 || end === -1) return text.trimEnd() + '\n\nOne more line, proposed.\n';
  return `${text.slice(0, start)}## Opening\n\n${NEW_OPENING}\n\n${text.slice(end)}`;
}

function sse(res, chunks) {
  res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' });
  for (const chunk of chunks) {
    res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: chunk } }] })}\n\n`);
  }
  res.write('data: [DONE]\n\n');
  res.end();
}

const server = createServer((req, res) => {
  if (req.method !== 'POST' || !req.url?.endsWith('/chat/completions')) {
    res.writeHead(404).end();
    return;
  }
  let raw = '';
  req.on('data', (d) => (raw += d));
  req.on('end', () => {
    let body = {};
    try {
      body = JSON.parse(raw);
    } catch {
      res.writeHead(400).end();
      return;
    }
    const note = attachedNote(body);
    if (!note) {
      sse(res, ['Attach a note and I can propose a change to it.']);
      return;
    }
    const birthday = note.text.includes('# Birthday ideas');
    const reply = [
      birthday
        ? 'Two of these are a tenner and two are a weekend. Splitting them makes the list easier to pick from.\n\n'
        : 'The opening reads as a list of what has stopped. Ending it on the shed and the tea turns that into a scene, and it sets up the recipe below.\n\n',
      `\`\`\`writ-proposal path="${note.path}" summary="${birthday ? 'Split the list by size' : 'End the opening on the shed'}"\n`,
      revise(note.text),
      '```\n',
    ];
    sse(res, reply);
  });
});

server.listen(port, '127.0.0.1', () => {
  console.log(`stub host on http://127.0.0.1:${port}/v1`);
});
