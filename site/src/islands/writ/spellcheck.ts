import type { SpellingLint } from '@app/types/spelling';

// A small, honest demo dictionary: common misspellings and their corrections.
// The real app runs a Rust engine; the demo only needs enough to show the
// status-bar states and "Fix all" working on a seeded buffer.
const CORRECTIONS: Record<string, string> = {
  teh: 'the',
  recieve: 'receive',
  seperate: 'separate',
  definately: 'definitely',
  occured: 'occurred',
  untill: 'until',
  wich: 'which',
  becuase: 'because',
  thier: 'their',
  enviroment: 'environment',
  dependancy: 'dependency',
  existance: 'existence',
  neccessary: 'necessary',
  accross: 'across',
  arguement: 'argument',
  commited: 'committed',
  occassionally: 'occasionally',
  refered: 'referred',
  succesful: 'successful',
  tommorow: 'tomorrow',
  wierd: 'weird',
  calender: 'calendar',
  garantee: 'guarantee',
  mispelled: 'misspelled',
  lenght: 'length',
  threshhold: 'threshold',
};

// Masks fenced code, inline code, and bare URLs with same-length runs of
// spaces so their offsets are preserved but no word token inside them is
// scanned. Word offsets read off the mask line up with the original document.
function maskCodeAndUrls(text: string): string {
  const blank = (m: string): string => ' '.repeat(m.length);
  return text
    .replace(/```[\s\S]*?```/g, blank)
    .replace(/`[^`\n]*`/g, blank)
    .replace(/\bhttps?:\/\/\S+/g, blank);
}

// Matches the surface case of the source word: Capitalized -> Capitalized,
// ALLCAPS -> ALLCAPS, otherwise lowercase.
function matchCase(source: string, correction: string): string {
  if (source === source.toUpperCase()) return correction.toUpperCase();
  if (source[0] === source[0]?.toUpperCase()) {
    return correction[0]!.toUpperCase() + correction.slice(1);
  }
  return correction;
}

const WORD = /[A-Za-z][A-Za-z']*/g;

export function checkSpelling(text: string): SpellingLint[] {
  const masked = maskCodeAndUrls(text);
  const lints: SpellingLint[] = [];
  for (const match of masked.matchAll(WORD)) {
    const word = match[0];
    const correction = CORRECTIONS[word.toLowerCase()];
    if (!correction) continue;
    const from = match.index;
    lints.push({
      fromUtf16: from,
      toUtf16: from + word.length,
      kind: 'spelling',
      message: `Did you mean "${matchCase(word, correction)}"?`,
      suggestions: [matchCase(word, correction)],
      confident: true,
    });
  }
  return lints;
}
