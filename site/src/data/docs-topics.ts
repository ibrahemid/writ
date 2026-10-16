export interface DocsTopic {
  /** The anchor the topic had on the one-page /docs/; its row on the index keeps it. */
  id: string;
  /** The page at /docs/{slug}/, or null for a topic a guide covers. */
  slug: string | null;
  /** Where a topic with no page of its own links, from the site root. */
  href?: string;
  title: string;
  description: string;
}

export interface DocsPage {
  title: string;
  description: string;
  path: string;
}

export class DocsTopicError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'DocsTopicError';
  }
}

/** The topics in the order the /docs/ index lists them. */
export const DOCS_TOPICS: readonly DocsTopic[] = [
  { id: 'install', slug: 'install', title: 'Install', description: 'The download page and the package managers that carry Writ.' },
  { id: 'first-launch', slug: 'first-launch', title: 'First launch', description: 'Choosing the format for new files and the apps on the first screen.' },
  { id: 'hotkey', slug: 'hotkey', title: 'The hotkey and quick capture', description: "The window's hotkey, the quick capture window, and opening at login." },
  { id: 'files', slug: 'files', title: 'Files and the folder', description: "Opening and saving files, Writ's folder, large files, text encodings, tabs and split panes." },
  { id: 'editing', slug: 'editing', title: 'Editing', description: 'Folding, the text transforms, and copying text as a prompt.' },
  { id: 'markdown', slug: 'markdown', title: 'Markdown', description: 'Inline and Source, what is drawn in place, and links between files.' },
  { id: 'preview', slug: 'preview', title: 'HTML preview', description: 'Source, Split and Preview for HTML and Mermaid files, and the scripts in a file.' },
  { id: 'search', slug: 'search', title: 'Search', description: 'Search Everywhere and its prefixes, the search field, and what search reads.' },
  { id: 'versions', slug: 'versions', title: 'Versions', description: 'The versions Writ keeps of a file, and how Revert To puts one back.' },
  { id: 'default-app', slug: 'default-app', title: 'Default app', description: 'Making Writ the app that opens a file type.' },
  { id: 'cli', slug: 'cli', title: 'Command line and links', description: 'The writ command and writ:// links.' },
  { id: 'inbox', slug: 'watched-folder', title: 'Watched folder', description: 'A folder whose new files open in Writ as they arrive.' },
  { id: 'spelling', slug: 'spelling', title: 'Spelling', description: 'Spell check and your dictionary.' },
  { id: 'apps', slug: 'apps', title: 'Apps', description: 'Connections, Graph, Tags, Chat, Rewrite and Connected programs, and the AI connection Chat and Rewrite share.' },
  { id: 'obsidian', slug: null, href: '/guides/obsidian-vault-files/', title: 'A folder written in Obsidian', description: 'Opening the folder Obsidian uses, and what carries over.' },
  { id: 'appearance', slug: 'appearance', title: 'Appearance', description: 'Themes, accents, interface text size and zoom.' },
  { id: 'shortcuts', slug: 'shortcuts', title: 'Shortcuts', description: 'The window and editor shortcuts, and which of them can be changed.' },
  { id: 'config', slug: 'config', title: 'Config', description: 'The config file, the logs and the update check.' },
];

/** The link a topic's row on the index carries. */
export function getDocsTopicHref(topic: DocsTopic, base: string): string {
  if (topic.slug !== null) return `${base}/docs/${topic.slug}/`;
  if (topic.href === undefined) {
    throw new DocsTopicError(`docs topic "${topic.id}" has neither a page nor a link`);
  }
  return `${base}${topic.href}`;
}

/** The title, description and path of the page a topic has under /docs/. */
export function getDocsPage(id: string): DocsPage {
  const topic = DOCS_TOPICS.find((t) => t.id === id);
  if (topic === undefined) throw new DocsTopicError(`no docs topic has the id "${id}"`);
  if (topic.slug === null) throw new DocsTopicError(`docs topic "${id}" has no page of its own`);
  return { title: topic.title, description: topic.description, path: `/docs/${topic.slug}/` };
}
