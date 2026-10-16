import { describe, it, expect } from 'vitest';
import {
  LLMS_DROP_SELECTORS,
  LlmsTxtError,
  decodeEntities,
  htmlToMarkdown,
  isStub,
  linkedPaths,
  orderSection,
  pageMeta,
  renderLlmsFull,
  renderLlmsTxt,
  type LlmsSite,
} from '../lib/llms-text';

const ORIGIN = 'https://writ.ibrahemid.com';
const PAGE = `${ORIGIN}/docs/`;

const page = (body: string, head = '<title>How Writ works | Writ</title><meta name="description" content="Files &amp; folders.">'): string =>
  `<!DOCTYPE html><html lang="en"><head>${head}</head><body><a href="#content" class="skip">Skip to content</a><main id="content" class="wrap"><article class="prose"><h1>How Writ works</h1>${body}</article></main></body></html>`;

const md = (body: string): string => htmlToMarkdown(page(body), PAGE);

describe('htmlToMarkdown', () => {
  it('leaves out the H1 and moves H2 and H3 down a level', () => {
    expect(md('<h2 id="files">Files</h2><h3>Encodings</h3>')).toBe('### Files\n\n#### Encodings');
  });

  it('keeps paragraphs apart and reads nothing outside main#content', () => {
    expect(md('<p>One.</p><p>Two.</p>')).toBe('One.\n\nTwo.');
  });

  it('marks strong text and inline code', () => {
    expect(md('<p>Run <code>writ --title tests</code> and read <strong>Program activity</strong>.</p>')).toBe(
      'Run `writ --title tests` and read **Program activity**.',
    );
  });

  it('writes a run of keycaps as the glyphs the page shows', () => {
    expect(md('<p>Press <kbd class="kc">⌘</kbd><kbd class="kc">⇧</kbd><kbd class="kc">V</kbd> to switch.</p>')).toBe('Press ⌘⇧V to switch.');
  });

  it('makes a relative link absolute against the page', () => {
    expect(md('<p>See <a href="/guides/obsidian/">the guide</a> and <a href="#cli">the command</a>.</p>')).toBe(
      `See [the guide](${ORIGIN}/guides/obsidian/) and [the command](${ORIGIN}/docs/#cli).`,
    );
  });

  it('numbers an ordered list from its start and bullets an unordered one', () => {
    expect(md('<ol><li>Open Settings.</li><li>Choose Apps.</li></ol><ul><li>Chat</li><li>Rewrite</li></ul>')).toBe(
      '1. Open Settings.\n2. Choose Apps.\n\n- Chat\n- Rewrite',
    );
    expect(md('<ol start="3"><li>Third</li><li>Fourth</li></ol>')).toBe('3. Third\n4. Fourth');
  });

  it('indents a nested list under its item', () => {
    expect(md('<ul><li>Apps<ul><li>Graph</li><li>Tags</li></ul></li></ul>')).toBe('- Apps\n  - Graph\n  - Tags');
  });

  it('keeps a code block verbatim inside a fence', () => {
    const json = '{\n  &quot;mcpServers&quot;: {\n    &quot;writ&quot;: {\n      &quot;args&quot;: [&quot;mcp&quot;]\n    }\n  }\n}';
    expect(md(`<pre><code>${json}</code></pre>`)).toBe(
      '```\n{\n  "mcpServers": {\n    "writ": {\n      "args": ["mcp"]\n    }\n  }\n}\n```',
    );
    expect(md('<pre><code>make test 2>&1 | writ --title tests</code></pre>')).toBe('```\nmake test 2>&1 | writ --title tests\n```');
  });

  it('lengthens the fence when the code holds one', () => {
    expect(md('<pre><code>```md\n# Title\n```</code></pre>')).toBe('````\n```md\n# Title\n```\n````');
  });

  it('writes a table as a Markdown table, with an empty header when no row is a header', () => {
    expect(md('<table><tbody><tr><th>macOS</th><td>Signed.</td></tr><tr><th>Linux</th><td>A | B</td></tr></tbody></table>')).toBe(
      '| | |\n| --- | --- |\n| macOS | Signed. |\n| Linux | A \\| B |',
    );
    expect(md('<table><thead><tr><th>Action</th><th>Keys</th></tr></thead><tbody><tr><td>Save</td><td><kbd class="kc">⌘</kbd><kbd class="kc">S</kbd></td></tr></tbody></table>')).toBe(
      '| Action | Keys |\n| --- | --- |\n| Save | ⌘S |',
    );
  });

  it('escapes a backslash in a table cell before the pipe that follows it', () => {
    expect(md('<table><tbody><tr><th>Path</th><td>C:\\Users\\ | B</td></tr></tbody></table>')).toBe(
      '| | |\n| --- | --- |\n| Path | C:\\\\Users\\\\ \\| B |',
    );
  });

  it('decodes named and numeric character references', () => {
    expect(md('<p>Files &amp; folders &rarr; disk,&nbsp;Writ&#39;s &#8220;folder&#8221; &#x2192; done.</p>')).toBe(
      'Files & folders → disk, Writ\'s “folder” → done.',
    );
  });

  it('drops every element on the drop list', () => {
    const dropped = [
      '<figure class="window"><figcaption>caption text</figcaption></figure>',
      '<picture><source srcset="/a.avif"></picture>',
      '<img src="/a.png" alt="alt text">',
      '<video><source src="/a.webm"></video>',
      '<script>window.x = "script text"</script>',
      '<style>.x{}</style>',
      '<button type="button">Copy</button>',
      '<nav class="toc"><a href="#install">Install</a></nav>',
      '<p hidden>hidden text</p>',
      '<span role="status">Copied</span>',
    ];
    expect(dropped).toHaveLength(LLMS_DROP_SELECTORS.length);
    expect(md(`<p>Kept.</p>${dropped.join('')}`)).toBe('Kept.');
  });

  it('keeps the text around controls it drops', () => {
    expect(
      md('<div class="dl-manager"><div class="dl-tool"><span class="dl-tool-name">Homebrew, in Terminal</span><span class="dl-status" role="status"></span><button type="button" hidden>Copy</button></div><code class="dl-cmd"><span>brew</span> <span>install</span></code></div>'),
    ).toBe('Homebrew, in Terminal\n\n`brew install`');
  });

  it('fails with the page named when it has no main#content', () => {
    expect(() => htmlToMarkdown('<html><body><p>x</p></body></html>', PAGE)).toThrow(LlmsTxtError);
    expect(() => htmlToMarkdown('<html><body><p>x</p></body></html>', PAGE)).toThrow(PAGE);
  });
});

describe('decodeEntities', () => {
  it('decodes in one pass', () => {
    expect(decodeEntities('&amp;lt;')).toBe('&lt;');
  });

  it('refuses a reference it cannot decode', () => {
    expect(() => decodeEntities('&bogus;')).toThrow(LlmsTxtError);
  });
});

describe('pageMeta', () => {
  it('reads the title without the site suffix and the decoded description', () => {
    expect(pageMeta(page(''), PAGE)).toEqual({ title: 'How Writ works', description: 'Files & folders.' });
  });

  it('fails when a page has no description', () => {
    expect(() => pageMeta(page('', '<title>Docs | Writ</title>'), PAGE)).toThrow(LlmsTxtError);
  });
});

describe('isStub', () => {
  it('knows the /vs/obsidian/ refresh from a page', () => {
    const stub =
      '<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><title>Writ and Obsidian on one folder | Writ</title><meta name="robots" content="noindex"><link rel="canonical" href="https://writ.ibrahemid.com/guides/obsidian/"><meta http-equiv="refresh" content="0; url=/guides/obsidian/"></head><body><a href="/guides/obsidian/">Writ and Obsidian on one folder</a></body></html>';
    expect(isStub(stub)).toBe(true);
    expect(isStub(page('<p>x</p>'))).toBe(false);
  });
});

describe('linkedPaths and orderSection', () => {
  it('reads the matching links of main#content in page order, once each, on this site only', () => {
    const html = page(
      '<ul><li id="files"><a href="/docs/files/" data-topic>Files</a></li><li id="obsidian"><a href="/guides/obsidian-vault-files/" data-topic>Obsidian</a></li><li><a href="/docs/files/" data-topic>Again</a></li><li><a href="https://github.com/ibrahemid/writ" data-topic>Code</a></li><li><a href="/docs/cli/">Not a topic</a></li></ul>',
    );
    expect(linkedPaths(html, PAGE, 'a[data-topic]')).toEqual(['/docs/files/', '/guides/obsidian-vault-files/']);
  });

  it('puts the index first, then the linked pages in link order, then the rest by path', () => {
    const routes = ['/docs/zeta/', '/docs/', '/docs/cli/', '/docs/files/', '/docs/alpha/'];
    expect(orderSection('/docs/', routes, ['/docs/files/', '/guides/obsidian-vault-files/', '/docs/cli/'])).toEqual([
      '/docs/',
      '/docs/files/',
      '/docs/cli/',
      '/docs/alpha/',
      '/docs/zeta/',
    ]);
  });

  it('fails when the index was not built', () => {
    expect(() => orderSection('/docs/', ['/docs/cli/'], [])).toThrow(LlmsTxtError);
  });
});

describe('renderLlmsTxt and renderLlmsFull', () => {
  const link = (path: string, title: string) => ({ url: `${ORIGIN}${path}`, title, description: `About ${title}.`, markdown: `Body of ${title}.` });
  const site: LlmsSite = {
    name: 'Writ',
    summary: 'A text app.',
    repository: 'https://github.com/ibrahemid/writ',
    docs: [link('/docs/', 'How Writ works')],
    guides: [link('/guides/', 'Guides')],
    reference: [link('/privacy/', 'Privacy'), link('/download/', 'Download')],
    optional: [link('/changelog/', 'Changelog')],
  };

  it('lists every page under its section, the optional ones last', () => {
    expect(renderLlmsTxt(site)).toBe(
      [
        '# Writ',
        '',
        '> A text app.',
        '',
        'Source, issues and releases: https://github.com/ibrahemid/writ',
        '',
        '## Docs',
        '',
        `- [How Writ works](${ORIGIN}/docs/): About How Writ works.`,
        '',
        '## Guides',
        '',
        `- [Guides](${ORIGIN}/guides/): About Guides.`,
        '',
        '## Reference',
        '',
        `- [Privacy](${ORIGIN}/privacy/): About Privacy.`,
        `- [Download](${ORIGIN}/download/): About Download.`,
        '',
        '## Optional',
        '',
        `- [Changelog](${ORIGIN}/changelog/): About Changelog.`,
        '',
      ].join('\n'),
    );
  });

  it('carries the body of every required page and none of the optional ones', () => {
    const full = renderLlmsFull(site);
    expect(full.startsWith('# Writ\n\n> A text app.\n\n## How Writ works\n\nSource: https://writ.ibrahemid.com/docs/\n\nBody of How Writ works.\n')).toBe(true);
    for (const title of ['Guides', 'Privacy', 'Download']) expect(full).toContain(`## ${title}\n\nSource: `);
    expect(full).not.toContain('Changelog');
  });
});
