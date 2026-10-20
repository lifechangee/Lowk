// In-page layout lint. `lintInPage` is serialised by Playwright: it must be self-contained.
export interface LintOpts {
  /** CSS selector of the subtree to lint (default body). */
  root?: string;
  /** Selectors excluded from the lint (dev harness chrome). */
  ignore?: string[];
  /** Selectors of key containers that must sit on the 4px grid. */
  grid?: string[];
  /** Selectors checked for size/padding/gap only (centered items have fractional offsets). */
  gridSize?: string[];
  /** Selectors checked for padding/gap only. */
  gridPad?: string[];
}
export interface Issue {
  rule: 'overflow' | 'outside' | 'text-overlap' | 'box-overlap' | 'flex-gap' | 'grid' | 'dup-words';
  sel: string;
  msg: string;
  rect?: { x: number; y: number; w: number; h: number };
}

export function lintInPage(opts: LintOpts): Issue[] {
  const out: Issue[] = [];
  const root = (opts.root ? document.querySelector(opts.root) : document.body) as HTMLElement | null;
  if (!root) return [{ rule: 'outside', sel: opts.root ?? 'body', msg: 'lint root not found' }];
  const ignore = opts.ignore ?? [];
  const skip = (el: Element) => ignore.some((s) => el.closest(s));
  const vw = document.documentElement.clientWidth;
  const vh = document.documentElement.clientHeight;
  const name = (el: Element): string => {
    const e = el as HTMLElement;
    let s = e.tagName.toLowerCase();
    if (e.id) s += '#' + e.id;
    const cls = [...(e.classList ?? [])].filter((c) => !c.startsWith('svelte-')).slice(0, 2);
    if (cls.length) s += '.' + cls.join('.');
    const p = e.parentElement;
    if (p && p !== root) {
      let ps = p.tagName.toLowerCase();
      const pc = [...p.classList].filter((c) => !c.startsWith('svelte-'))[0];
      if (pc) ps += '.' + pc;
      s = ps + ' > ' + s;
    }
    return s;
  };
  const visible = (el: Element) => {
    const cs = getComputedStyle(el);
    if (cs.display === 'none' || cs.visibility === 'hidden' || Number(cs.opacity) === 0) return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const all = [...root.querySelectorAll('*')].filter((e) => !skip(e) && !['SCRIPT', 'STYLE', 'svg', 'path', 'g', 'circle', 'rect', 'line', 'polyline'].includes(e.tagName) && visible(e));

  // Own text rect (union of the element's direct, non-blank text nodes).
  const textRect = (el: Element): DOMRect | null => {
    let u: DOMRect | null = null;
    for (const n of el.childNodes) {
      if (n.nodeType !== 3 || !n.textContent?.trim()) continue;
      const rg = document.createRange();
      rg.selectNodeContents(n);
      for (const r of rg.getClientRects()) {
        if (r.width < 1 || r.height < 1) continue;
        u = u ? new DOMRect(Math.min(u.left, r.left), Math.min(u.top, r.top), Math.max(u.right, r.right) - Math.min(u.left, r.left), Math.max(u.bottom, r.bottom) - Math.min(u.top, r.top)) : r;
      }
    }
    return u;
  };
  const rectOf = (r: DOMRect) => ({ x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height) });
  const clipAncestors = (el: Element): HTMLElement[] => {
    const res: HTMLElement[] = [];
    for (let p = el.parentElement; p; p = p.parentElement) {
      const cs = getComputedStyle(p);
      if (cs.overflowX !== 'visible' || cs.overflowY !== 'visible') res.push(p);
    }
    return res;
  };

  const texts: { el: Element; r: DOMRect }[] = [];
  for (const el of all) {
    const tr = textRect(el);
    if (!tr) continue;
    const h = el as HTMLElement;
    const cs = getComputedStyle(h);
    // 1) clipped text: own box clips horizontally without an ellipsis.
    if (cs.overflowX !== 'visible' && cs.overflowX !== 'auto' && cs.overflowX !== 'scroll' && h.scrollWidth > h.clientWidth + 1 && cs.textOverflow !== 'ellipsis') {
      out.push({ rule: 'overflow', sel: name(el), msg: `text clipped without ellipsis (scrollWidth ${h.scrollWidth} > clientWidth ${h.clientWidth})`, rect: rectOf(tr) });
    }
    // 2) walk the clipping ancestors. A scroll container on an axis legitimises everything beyond
    //    it on that axis (the user can scroll); the visible part is what remains after clipping.
    let vis = { l: tr.left, t: tr.top, r: tr.right, b: tr.bottom };
    let clippedBy: string | null = null;
    let xScrolls = false, yScrolls = false;
    for (const a of clipAncestors(el)) {
      if (a === document.body || a === document.documentElement) continue;
      const ar = a.getBoundingClientRect();
      const acs = getComputedStyle(a);
      const sx = acs.overflowX === 'auto' || acs.overflowX === 'scroll';
      const sy = acs.overflowY === 'auto' || acs.overflowY === 'scroll';
      if (!xScrolls && !sx && (tr.right > ar.right + 1 || tr.left < ar.left - 1) && cs.textOverflow !== 'ellipsis') clippedBy ??= name(a) + ' (x)';
      if (!yScrolls && !sy && (tr.bottom > ar.bottom + 1 || tr.top < ar.top - 1)) clippedBy ??= name(a) + ' (y)';
      if (sx) xScrolls = true;
      if (sy) yScrolls = true;
      vis = { l: Math.max(vis.l, ar.left), t: Math.max(vis.t, ar.top), r: Math.min(vis.r, ar.right), b: Math.min(vis.b, ar.bottom) };
    }
    if (clippedBy) out.push({ rule: 'overflow', sel: name(el), msg: `text exceeds clipping ancestor ${clippedBy}`, rect: rectOf(tr) });
    if (vis.r - vis.l < 1 || vis.b - vis.t < 1) continue; // scrolled / clipped away entirely
    if (vis.r > vw + 1 || vis.b > vh + 1 || vis.l < -1 || vis.t < -1) out.push({ rule: 'outside', sel: name(el), msg: `text outside the viewport ${vw}x${vh}`, rect: rectOf(tr) });
    texts.push({ el, r: new DOMRect(vis.l, vis.t, vis.r - vis.l, vis.b - vis.t) });
  }

  // 3) overlapping text from unrelated elements.
  for (let i = 0; i < texts.length; i++) {
    for (let j = i + 1; j < texts.length; j++) {
      const a = texts[i]!, b = texts[j]!;
      if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
      const pop = '.menu, [role=menu], [role=dialog], [role=listbox], .modal, .pop';
      if (!!a.el.closest(pop) !== !!b.el.closest(pop)) continue; // popovers legitimately cover content
      const w = Math.min(a.r.right, b.r.right) - Math.max(a.r.left, b.r.left);
      const h = Math.min(a.r.bottom, b.r.bottom) - Math.max(a.r.top, b.r.top);
      if (w > 2 && h > 3) out.push({ rule: 'text-overlap', sel: `${name(a.el)}  <>  ${name(b.el)}`, msg: `text boxes overlap ${Math.round(w)}x${Math.round(h)}px`, rect: rectOf(a.r) });
    }
  }

  // 4) in-flow siblings whose boxes overlap.
  const parents = new Set(all.map((e) => e.parentElement).filter(Boolean) as HTMLElement[]);
  for (const p of parents) {
    const kids = [...p.children].filter((k) => all.includes(k) && ['static', 'relative'].includes(getComputedStyle(k).position));
    for (let i = 0; i < kids.length; i++) {
      for (let j = i + 1; j < kids.length; j++) {
        const a = kids[i]!.getBoundingClientRect(), b = kids[j]!.getBoundingClientRect();
        const w = Math.min(a.right, b.right) - Math.max(a.left, b.left);
        const h = Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top);
        if (w > 1.5 && h > 1.5) out.push({ rule: 'box-overlap', sel: `${name(kids[i]!)}  <>  ${name(kids[j]!)}`, msg: `sibling boxes overlap ${Math.round(w)}x${Math.round(h)}px`, rect: rectOf(a) });
      }
    }
  }

  // 5) chips (dot, icon, kbd, bordered/filled boxes) that touch their neighbouring text/chip on a line.
  //    Catches «●Новый созвон⇧⌘R»: inline children of a span with no gap/margin/space between them.
  const isChip = (el: Element): boolean => {
    const cs = getComputedStyle(el);
    if (['KBD', 'SVG', 'IMG', 'svg'].includes(el.tagName)) return true;
    const bw = parseFloat(cs.borderTopWidth) + parseFloat(cs.borderLeftWidth);
    const bg = cs.backgroundColor;
    const filled = bg !== 'rgba(0, 0, 0, 0)' && bg !== 'transparent';
    const r = el.getBoundingClientRect();
    if (r.width > 220 || r.height > 56) return false; // panels are not chips
    return bw > 0 || filled || (r.width <= 24 && r.height <= 24 && !el.textContent?.trim());
  };
  for (const el of all) {
    const items: { r: DOMRect; chip: boolean; label: string }[] = [];
    for (const n of el.childNodes) {
      if (n.nodeType === 3 && n.textContent?.trim()) {
        const rg = document.createRange();
        rg.selectNodeContents(n);
        const r = rg.getBoundingClientRect();
        if (r.width > 0) items.push({ r, chip: false, label: 'text' });
      } else if (n.nodeType === 1 && visible(n as Element) && getComputedStyle(n as Element).position !== 'absolute') {
        const c = n as Element;
        items.push({ r: c.getBoundingClientRect(), chip: isChip(c), label: c.tagName.toLowerCase() + (c.classList[0] ? '.' + c.classList[0] : '') });
      }
    }
    if (items.length < 2) continue;
    items.sort((a, b) => a.r.left - b.r.left);
    for (let i = 0; i < items.length - 1; i++) {
      const a = items[i]!, b = items[i + 1]!;
      const vOverlap = Math.min(a.r.bottom, b.r.bottom) - Math.max(a.r.top, b.r.top);
      const dist = b.r.left - a.r.right;
      if ((a.chip || b.chip) && vOverlap > 4 && dist < 2 && dist > -1 && a.r.width > 3 && b.r.width > 3) {
        out.push({ rule: 'flex-gap', sel: name(el), msg: `no spacing between "${a.label}" and "${b.label}" (${dist.toFixed(1)}px)`, rect: rectOf(el.getBoundingClientRect()) });
      }
    }
  }

  // 6) key containers on the 4px grid (position, size where explicit, padding/gap).
  const on4 = (v: number) => Math.abs(v / 4 - Math.round(v / 4)) < 0.05;
  const gridSets: [string, 'pos' | 'size' | 'pad'][] = [...(opts.grid ?? []).map((g) => [g, 'pos'] as [string, 'pos']), ...(opts.gridSize ?? []).map((g) => [g, 'size'] as [string, 'size']), ...(opts.gridPad ?? []).map((g) => [g, 'pad'] as [string, 'pad'])];
  for (const [sel, kind] of gridSets) {
    const pos = kind === 'pos';
    for (const el of root.querySelectorAll(sel)) {
      if (skip(el) || !visible(el)) continue;
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      const bad: string[] = [];
      if (pos && !on4(r.top)) bad.push(`top=${r.top.toFixed(1)}`);
      if (pos && !on4(r.left)) bad.push(`left=${r.left.toFixed(1)}`);
      if (kind !== 'pad' && !on4(r.height)) bad.push(`height=${r.height.toFixed(1)}`);
      for (const p of ['paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'rowGap', 'columnGap'] as const) {
        const v = parseFloat(cs[p]);
        if (v && !on4(v)) bad.push(`${p}=${v}`);
      }
      if (bad.length) out.push({ rule: 'grid', sel: name(el), msg: `off the 4px grid: ${bad.join(', ')}`, rect: rectOf(r) });
    }
  }
  return out;
}

/** Consecutive duplicated words («Это Это подсказка-заглушка: подсказка-заглушка:») in a text. */
export function dupWords(text: string): string[] {
  const words = text.split(/\s+/).filter(Boolean);
  const hits: string[] = [];
  for (let i = 1; i < words.length; i++) {
    const a = words[i - 1]!.toLowerCase(), b = words[i]!.toLowerCase();
    if (a === b && a.replace(/[^\p{L}\p{N}]/gu, '').length >= 3) hits.push(words[i]!);
  }
  return hits;
}
