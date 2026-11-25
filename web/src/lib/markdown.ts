export type Inline =
  | { kind: 'text' | 'code' | 'strong' | 'em' | 'del'; text: string }
  | { kind: 'link'; text: string; href: string };

export type Block =
  | { kind: 'paragraph' | 'quote'; inline: Inline[] }
  | { kind: 'heading'; level: number; inline: Inline[] }
  | { kind: 'code'; lang: string; code: string }
  | { kind: 'list'; ordered: boolean; items: Inline[][] }
  | { kind: 'hr' };

const UNSAFE_URL = /^(javascript|data|vbscript):/i;

function safeHref(href: string): string | null {
  const h = href.trim();
  if (UNSAFE_URL.test(h)) return null;
  if (/^(https?:\/\/|mailto:|\/|#|\.\/)/i.test(h)) return h;
  return null;
}

/** Small, deliberately conservative inline parser. Everything is escaped by Svelte. */
export function parseInline(source: string): Inline[] {
  const out: Inline[] = [];
  const re = /(`[^`\n]+`)|(\*\*[^*\n]+\*\*)|(\*[^*\n]+\*)|(~~[^~\n]+~~)|(\[[^\]\n]+\]\([^)\n]+\))/g;
  let last = 0;
  let match: RegExpExecArray | null;
  while ((match = re.exec(source))) {
    if (match.index > last) out.push({ kind: 'text', text: source.slice(last, match.index) });
    const token = match[0];
    if (token.startsWith('`')) out.push({ kind: 'code', text: token.slice(1, -1) });
    else if (token.startsWith('**')) out.push({ kind: 'strong', text: token.slice(2, -2) });
    else if (token.startsWith('~~')) out.push({ kind: 'del', text: token.slice(2, -2) });
    else if (token.startsWith('*')) out.push({ kind: 'em', text: token.slice(1, -1) });
    else {
      const split = token.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
      if (split) {
        const href = safeHref(split[2]);
        if (href) out.push({ kind: 'link', text: split[1], href });
        else out.push({ kind: 'text', text: split[1] });
      }
    }
    last = match.index + token.length;
  }
  if (last < source.length) out.push({ kind: 'text', text: source.slice(last) });
  return out;
}

export function parseMarkdown(source: string): Block[] {
  const lines = String(source || '').replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
      continue;
    }

    const fence = line.match(/^\s*(`{3,}|~{3,})\s*([\w.+-]*)\s*$/);
    if (fence) {
      const marker = fence[1][0];
      const length = fence[1].length;
      const lang = fence[2] || '';
      const code: string[] = [];
      i++;
      while (i < lines.length) {
        const candidate = lines[i].match(new RegExp(`^\\s*${marker === '`' ? '\\`' : marker}{${length},}\\s*$`));
        if (candidate) {
          i++;
          break;
        }
        code.push(lines[i]);
        i++;
      }
      blocks.push({ kind: 'code', lang, code: code.join('\n') });
      continue;
    }

    if (/^\s*([-*_])\s*\1\s*\1[\s-*_]*$/.test(line)) {
      blocks.push({ kind: 'hr' });
      i++;
      continue;
    }

    const heading = line.match(/^\s*(#{1,6})\s+(.*)$/);
    if (heading) {
      blocks.push({ kind: 'heading', level: heading[1].length, inline: parseInline(heading[2]) });
      i++;
      continue;
    }

    if (/^\s*>/.test(line)) {
      const quote: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) {
        quote.push(lines[i].replace(/^\s*>\s?/, ''));
        i++;
      }
      blocks.push({ kind: 'quote', inline: parseInline(quote.join(' ')) });
      continue;
    }

    const bullet = line.match(/^\s*([-*+]|\d+[.)])\s+(.*)$/);
    if (bullet) {
      const ordered = /\d/.test(bullet[1]);
      const items: Inline[][] = [];
      while (i < lines.length) {
        const item = lines[i].match(/^\s*([-*+]|\d+[.)])\s+(.*)$/);
        if (!item || /\d/.test(item[1]) !== ordered) break;
        items.push(parseInline(item[2]));
        i++;
      }
      blocks.push({ kind: 'list', ordered, items });
      continue;
    }

    const paragraph: string[] = [line.trim()];
    i++;
    while (i < lines.length && lines[i].trim() && !/^\s*(`{3,}|~{3,}|#{1,6}\s|>|[-*+]\s|\d+[.)]\s)/.test(lines[i])) {
      paragraph.push(lines[i].trim());
      i++;
    }
    blocks.push({ kind: 'paragraph', inline: parseInline(paragraph.join(' ')) });
  }
  return blocks;
}
