import { ELEMENT_NODE, TEXT_NODE, parse, type ElementNode, type Node } from 'ultrahtml';
import { matches, querySelector, querySelectorAll } from 'ultrahtml/selector';

export class LlmsTxtError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'LlmsTxtError';
  }
}

export const LLMS_DROP_SELECTORS = [
  'figure',
  'picture',
  'img',
  'video',
  'script',
  'style',
  'button',
  'nav.toc',
  '[hidden]',
  '[role="status"]',
] as const;

export interface LlmsLink {
  url: string;
  title: string;
  description: string;
}

export interface LlmsPage extends LlmsLink {
  markdown: string;
}

export interface LlmsSite {
  name: string;
  summary: string;
  repository: string;
  docs: LlmsPage[];
  guides: LlmsPage[];
  reference: LlmsPage[];
  optional: LlmsLink[];
}

const NAMED_ENTITIES: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: ' ',
  thinsp: ' ',
  rarr: '→',
  larr: '←',
  hellip: '…',
  middot: '·',
  times: '×',
  copy: '©',
  lsquo: '‘',
  rsquo: '’',
  ldquo: '“',
  rdquo: '”',
  ndash: '–',
};

const MAX_CODE_POINT = 0x10ffff;

export function decodeEntities(text: string): string {
  return text.replace(/&(?:#(\d+)|#x([0-9a-f]+)|([a-z][a-z0-9]*));/gi, (whole, dec?: string, hex?: string, name?: string) => {
    if (name !== undefined) {
      const named = NAMED_ENTITIES[name];
      if (named === undefined) throw new LlmsTxtError(`unknown character reference ${whole}`);
      return named;
    }
    const code = dec !== undefined ? Number.parseInt(dec, 10) : Number.parseInt(hex ?? '', 16);
    if (!Number.isInteger(code) || code > MAX_CODE_POINT) throw new LlmsTxtError(`invalid character reference ${whole}`);
    return String.fromCodePoint(code);
  });
}

const isElement = (node: Node): node is ElementNode => node.type === ELEMENT_NODE;

const isDropped = (el: ElementNode): boolean => LLMS_DROP_SELECTORS.some((selector) => matches(el, selector));

const collapse = (text: string): string => text.replace(/\s+/g, ' ');

function rawText(node: Node): string {
  if (node.type === TEXT_NODE) return decodeEntities(node.value);
  if (!isElement(node) || isDropped(node)) return '';
  if (node.name === 'br') return '\n';
  return node.children.map(rawText).join('');
}

function mainOf(html: string, url: string): ElementNode {
  const main = querySelector(parse(html), 'main#content');
  if (!main || !isElement(main)) throw new LlmsTxtError(`${url} has no main#content`);
  return main;
}

const BLOCKS = new Set([
  'address', 'article', 'aside', 'blockquote', 'dd', 'details', 'div', 'dl', 'dt', 'fieldset', 'footer', 'form',
  'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'header', 'hr', 'li', 'main', 'nav', 'ol', 'p', 'pre', 'section', 'summary',
  'table', 'ul',
]);

function fenceFor(text: string, min: number): string {
  const longest = Math.max(0, ...[...text.matchAll(/`+/g)].map((m) => m[0].length));
  return '`'.repeat(Math.max(min, longest + 1));
}

class Converter {
  private readonly pageUrl: string;

  constructor(pageUrl: string) {
    this.pageUrl = pageUrl;
  }

  inline(node: Node): string {
    if (node.type === TEXT_NODE) return collapse(decodeEntities(node.value));
    if (!isElement(node) || isDropped(node)) return '';
    const inner = (): string => node.children.map((child) => this.inline(child)).join('');
    switch (node.name) {
      case 'br':
        return '\n';
      case 'code': {
        const text = collapse(rawText(node));
        if (!text.trim()) return text;
        const fence = fenceFor(text, 1);
        const pad = text.startsWith('`') || text.endsWith('`') ? ' ' : '';
        return `${fence}${pad}${text}${pad}${fence}`;
      }
      case 'strong':
      case 'b':
        return this.wrap(inner(), '**');
      case 'a':
        return this.link(node, inner());
      default:
        return inner();
    }
  }

  private wrap(text: string, marker: string): string {
    const core = text.trim();
    if (!core) return text;
    const lead = text.startsWith(' ') ? ' ' : '';
    const tail = text.endsWith(' ') ? ' ' : '';
    return `${lead}${marker}${core}${marker}${tail}`;
  }

  private link(el: ElementNode, text: string): string {
    const href = decodeEntities(el.attributes.href ?? '').trim();
    const label = text.trim();
    if (!label) return text;
    if (!href || /^javascript:/i.test(href)) return text;
    const lead = text.startsWith(' ') ? ' ' : '';
    const tail = text.endsWith(' ') ? ' ' : '';
    return `${lead}[${label}](${new URL(href, this.pageUrl).href})${tail}`;
  }

  blocks(el: ElementNode): string[] {
    const out: string[] = [];
    let run = '';
    const flush = (): void => {
      const text = run.replace(/ *\n */g, '\n').replace(/ {2,}/g, ' ').trim();
      if (text) out.push(text);
      run = '';
    };
    for (const child of el.children) {
      if (isElement(child) && BLOCKS.has(child.name)) {
        if (isDropped(child)) continue;
        flush();
        out.push(...this.block(child));
      } else {
        run += this.inline(child);
      }
    }
    flush();
    return out;
  }

  private block(el: ElementNode): string[] {
    switch (el.name) {
      case 'h1':
        return [];
      case 'h2':
      case 'h3':
      case 'h4':
      case 'h5':
      case 'h6': {
        const text = collapse(this.blocks(el).join(' ')).trim();
        const level = Math.min(Number(el.name.slice(1)) + 1, 6);
        return text ? [`${'#'.repeat(level)} ${text}`] : [];
      }
      case 'p': {
        const text = this.blocks(el).join('\n');
        return text ? [text] : [];
      }
      case 'ul':
      case 'ol':
        return this.list(el);
      case 'pre':
        return [this.fenced(el)];
      case 'table':
        return this.table(el);
      case 'blockquote':
        return [this.blocks(el).join('\n\n').split('\n').map((line) => (line ? `> ${line}` : '>')).join('\n')];
      case 'hr':
        return ['---'];
      default:
        return this.blocks(el);
    }
  }

  private list(el: ElementNode): string[] {
    const ordered = el.name === 'ol';
    let n = ordered ? Number.parseInt(el.attributes.start ?? '1', 10) || 1 : 0;
    const items: string[] = [];
    for (const li of el.children) {
      if (!isElement(li) || li.name !== 'li' || isDropped(li)) continue;
      const marker = ordered ? `${n++}.` : '-';
      const indent = ' '.repeat(marker.length + 1);
      const parts = this.blocks(li);
      const body = parts
        .map((part, i) => (i === 0 ? part : `${/^(-|\d+\.) /.test(part) ? '\n' : '\n\n'}${part}`))
        .join('')
        .split('\n')
        .map((line, i) => (i === 0 || !line ? line : `${indent}${line}`))
        .join('\n');
      items.push(`${marker} ${body}`.trimEnd());
    }
    return items.length ? [items.join('\n')] : [];
  }

  private fenced(el: ElementNode): string {
    const code = el.children.find((child): child is ElementNode => isElement(child) && child.name === 'code');
    const lang = /(?:^|\s)language-([\w-]+)/.exec(code?.attributes.class ?? '')?.[1] ?? '';
    const text = rawText(el).replace(/\n$/, '');
    const fence = fenceFor(text, 3);
    return `${fence}${lang}\n${text}\n${fence}`;
  }

  private table(el: ElementNode): string[] {
    const rows: { cells: string[]; header: boolean }[] = [];
    const visit = (node: ElementNode, inHead: boolean): void => {
      for (const child of node.children) {
        if (!isElement(child) || isDropped(child)) continue;
        if (child.name === 'thead') visit(child, true);
        else if (child.name === 'tbody' || child.name === 'tfoot') visit(child, inHead);
        else if (child.name === 'tr') {
          const cells = child.children.filter(
            (cell): cell is ElementNode => isElement(cell) && (cell.name === 'th' || cell.name === 'td') && !isDropped(cell),
          );
          rows.push({
            cells: cells.map((cell) => collapse(this.blocks(cell).join(' ')).trim().replace(/[\\|]/g, '\\$&')),
            header: inHead || (cells.length > 0 && cells.every((cell) => cell.name === 'th')),
          });
        }
      }
    };
    visit(el, false);
    if (!rows.length) return [];
    const width = Math.max(...rows.map((row) => row.cells.length));
    const line = (cells: string[]): string =>
      `| ${Array.from({ length: width }, (_, i) => cells[i] ?? '').join(' | ')} |`.replace(/ {2,}/g, ' ');
    const first = rows[0];
    const head = first?.header ? first.cells : [];
    const body = first?.header ? rows.slice(1) : rows;
    const divider = `| ${Array.from({ length: width }, () => '---').join(' | ')} |`;
    return [[line(head), divider, ...body.map((row) => line(row.cells))].join('\n')];
  }
}

export function htmlToMarkdown(html: string, pageUrl: string): string {
  return new Converter(pageUrl).blocks(mainOf(html, pageUrl)).join('\n\n');
}

export function pageMeta(html: string, url: string): { title: string; description: string } {
  const doc = parse(html);
  const titleEl = querySelector(doc, 'title');
  const title = titleEl ? collapse(rawText(titleEl)).trim().replace(/\s*\|\s*Writ$/, '') : '';
  const meta = querySelector(doc, 'meta[name="description"]');
  const description = collapse(decodeEntities(meta && isElement(meta) ? (meta.attributes.content ?? '') : '')).trim();
  if (!title) throw new LlmsTxtError(`${url} has no <title>`);
  if (!description) throw new LlmsTxtError(`${url} has no meta description`);
  return { title, description };
}

export function isStub(html: string): boolean {
  return Boolean(querySelector(parse(html), 'meta[http-equiv="refresh"]'));
}

export function linkedPaths(html: string, pageUrl: string, selector: string): string[] {
  const origin = new URL(pageUrl).origin;
  const paths: string[] = [];
  for (const node of querySelectorAll(mainOf(html, pageUrl), selector)) {
    if (!isElement(node) || node.attributes.href === undefined) continue;
    const target = new URL(decodeEntities(node.attributes.href), pageUrl);
    if (target.origin === origin && !paths.includes(target.pathname)) paths.push(target.pathname);
  }
  return paths;
}

export function orderSection(index: string, routes: string[], linked: string[]): string[] {
  if (!routes.includes(index)) throw new LlmsTxtError(`${index} was not built`);
  const rest = routes.filter((route) => route !== index);
  const inLinkOrder = linked.filter((route) => rest.includes(route));
  const unlinked = rest.filter((route) => !inLinkOrder.includes(route)).sort();
  return [index, ...inLinkOrder, ...unlinked];
}

const oneLine = (text: string): string => collapse(text).trim();

const listLine = ({ url, title, description }: LlmsLink): string => `- [${oneLine(title)}](${url}): ${oneLine(description)}`;

const head = (site: LlmsSite): string => `# ${site.name}\n\n> ${oneLine(site.summary)}\n`;

export function renderLlmsTxt(site: LlmsSite): string {
  const sections: [string, LlmsLink[]][] = [
    ['Docs', site.docs],
    ['Guides', site.guides],
    ['Reference', site.reference],
    ['Optional', site.optional],
  ];
  return [
    head(site),
    `Source, issues and releases: ${site.repository}\n`,
    ...sections.map(([name, links]) => `## ${name}\n\n${links.map(listLine).join('\n')}\n`),
  ].join('\n');
}

export function renderLlmsFull(site: LlmsSite): string {
  const pages = [...site.docs, ...site.guides, ...site.reference];
  return [
    head(site),
    ...pages.map((page) => `## ${oneLine(page.title)}\n\nSource: ${page.url}\n\n${page.markdown}\n`),
  ].join('\n');
}
