// A deliberately tiny Markdown reader for the summary and for converted documents. It produces plain data
// (blocks and inline segments) that Svelte renders as text nodes: no HTML is ever injected.
export type Inline = { b?: boolean; code?: boolean; t: string };
export type Block =
  | { k: 'h'; level: number; text: Inline[] }
  | { k: 'p'; text: Inline[] }
  | { k: 'ul'; items: Inline[][] }
  | { k: 'ol'; items: Inline[][]; nums: number[] }
  | { k: 'quote'; text: Inline[] }
  | { k: 'code'; text: string }
  | { k: 'table'; head: Inline[][]; rows: Inline[][][] };

export function inline(s: string): Inline[] {
  const out: Inline[] = [];
  for (const [i, part] of s.split(/(\*\*[^*]+\*\*|`[^`]+`)/g).entries()) {
    if (!part) continue;
    if (part.startsWith('**') && part.endsWith('**') && part.length > 4) out.push({ b: true, t: part.slice(2, -2) });
    else if (part.startsWith('`') && part.endsWith('`') && part.length > 2) out.push({ code: true, t: part.slice(1, -1) });
    else out.push({ t: part });
    void i;
  }
  return out;
}

const cells = (line: string): string[] => line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map((c) => c.trim());
const isSep = (line: string) => /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(line);

export function blocks(md: string): Block[] {
  const lines = md.replace(/\r\n?/g, '\n').split('\n');
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    if (!line.trim() || /^<!--.*-->\s*$/.test(line)) { i++; continue; }
    const h = /^(#{1,6})\s+(.*)$/.exec(line);
    if (h) { out.push({ k: 'h', level: h[1]!.length, text: inline(h[2]!.trim()) }); i++; continue; }
    if (line.startsWith('```')) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i]!.startsWith('```')) body.push(lines[i++]!);
      i++;
      out.push({ k: 'code', text: body.join('\n') });
      continue;
    }
    if (line.trim().startsWith('|') && i + 1 < lines.length && isSep(lines[i + 1]!)) {
      const head = cells(line).map(inline);
      i += 2;
      const rows: Inline[][][] = [];
      while (i < lines.length && lines[i]!.trim().startsWith('|')) rows.push(cells(lines[i++]!).map(inline));
      out.push({ k: 'table', head, rows });
      continue;
    }
    if (/^\s*[-*]\s+/.test(line)) {
      const items: Inline[][] = [];
      while (i < lines.length && /^\s*[-*]\s+/.test(lines[i]!)) items.push(inline(lines[i++]!.replace(/^\s*[-*]\s+/, '')));
      out.push({ k: 'ul', items });
      continue;
    }
    if (/^\s*\d+[.)]\s+/.test(line)) {
      const items: Inline[][] = [];
      const nums: number[] = [];
      // keep the document's own numbers (a clause list that starts at 3, or skips): never renumber from 1
      while (i < lines.length && /^\s*\d+[.)]\s+/.test(lines[i]!)) {
        nums.push(Number(/^\s*(\d+)/.exec(lines[i]!)![1]));
        items.push(inline(lines[i++]!.replace(/^\s*\d+[.)]\s+/, '')));
      }
      out.push({ k: 'ol', items, nums });
      continue;
    }
    if (line.startsWith('>')) {
      const body: string[] = [];
      while (i < lines.length && lines[i]!.startsWith('>')) body.push(lines[i++]!.replace(/^>\s?/, ''));
      out.push({ k: 'quote', text: inline(body.join(' ')) });
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i]!.trim() && !/^(#{1,6}\s|```|\s*[-*]\s|\s*\d+[.)]\s|>|\|)/.test(lines[i]!) && !/^<!--.*-->\s*$/.test(lines[i]!)) para.push(lines[i++]!.trim());
    if (para.length) out.push({ k: 'p', text: inline(para.join(' ')) });
    else i++;
  }
  return out;
}
