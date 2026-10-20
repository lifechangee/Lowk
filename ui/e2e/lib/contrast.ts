import type { Page } from '@playwright/test';

export interface ContrastIssue {
  sel: string;
  text: string;
  ratio: number;
  need: number;
  color: string;
  rect: { x: number; y: number; w: number; h: number };
}

/**
 * Pixel-based WCAG contrast. Text is made transparent, the page is screenshotted (pure
 * background incl. blur/scrims/images), then each text box is compared with its computed colour
 * against the worst background pixels. Works over translucent overlays where DOM-only tools cannot.
 */
export async function measureContrast(page: Page, rootSel: string, ignore: string[] = []): Promise<ContrastIssue[]> {
  const boxes = await page.evaluate(
    ({ rootSel, ignore }) => {
      const root = document.querySelector(rootSel) ?? document.body;
      const res: { sel: string; text: string; color: string; size: number; weight: number; x: number; y: number; w: number; h: number }[] = [];
      for (const el of root.querySelectorAll('*')) {
        if (ignore.some((s) => el.closest(s))) continue;
        const cs = getComputedStyle(el);
        if (cs.display === 'none' || cs.visibility === 'hidden' || Number(cs.opacity) === 0) continue;
        let txt = '';
        const rg = document.createRange();
        let u: DOMRect | null = null;
        for (const n of el.childNodes) {
          if (n.nodeType !== 3 || !n.textContent?.trim()) continue;
          txt += n.textContent.trim() + ' ';
          rg.selectNodeContents(n);
          for (const r of rg.getClientRects()) if (r.width > 1 && r.height > 1) u = u ? new DOMRect(Math.min(u.left, r.left), Math.min(u.top, r.top), Math.max(u.right, r.right) - Math.min(u.left, r.left), Math.max(u.bottom, r.bottom) - Math.min(u.top, r.top)) : r;
        }
        if (!u) continue;
        // effective alpha: element opacity chain
        let op = 1;
        for (let p: Element | null = el; p; p = p.parentElement) op *= Number(getComputedStyle(p).opacity);
        const m = cs.color.match(/[\d.]+/g)!.map(Number);
        const a = (m[3] ?? 1) * op;
        const cls = [...el.classList].filter((c) => !c.startsWith('svelte-'))[0];
        res.push({ sel: el.tagName.toLowerCase() + (cls ? '.' + cls : ''), text: txt.trim().slice(0, 40), color: `${m[0]},${m[1]},${m[2]},${a}`, size: parseFloat(cs.fontSize), weight: Number(cs.fontWeight) || 400, x: u.x, y: u.y, w: u.width, h: u.height });
      }
      return res;
    },
    { rootSel, ignore },
  );
  if (!boxes.length) return [];
  const style = await page.addStyleTag({ content: '*{color:transparent !important;text-shadow:none !important;-webkit-text-fill-color:transparent !important}svg{visibility:hidden !important}' });
  const png = (await page.screenshot()).toString('base64');
  await style.evaluate((n) => n.remove());
  return page.evaluate(
    async ({ png, boxes }) => {
      const img = new Image();
      img.src = 'data:image/png;base64,' + png;
      await img.decode();
      const c = document.createElement('canvas');
      c.width = img.width;
      c.height = img.height;
      const g = c.getContext('2d', { willReadFrequently: true })!;
      g.drawImage(img, 0, 0);
      const lin = (v: number) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
      const lum = (r: number, gg: number, b: number) => 0.2126 * lin(r) + 0.7152 * lin(gg) + 0.0722 * lin(b);
      const out: { sel: string; text: string; ratio: number; need: number; color: string; rect: { x: number; y: number; w: number; h: number } }[] = [];
      for (const b of boxes) {
        const x = Math.max(0, Math.floor(b.x)), y = Math.max(0, Math.floor(b.y));
        const w = Math.min(img.width - x, Math.ceil(b.w)), h = Math.min(img.height - y, Math.ceil(b.h));
        if (w < 2 || h < 2) continue;
        const d = g.getImageData(x, y, w, h).data;
        const [tr, tg, tb, ta] = b.color.split(',').map(Number) as [number, number, number, number];
        const ratios: number[] = [];
        const step = Math.max(1, Math.floor((w * h) / 400));
        for (let i = 0; i < w * h; i += step) {
          const r = d[i * 4]!, gg = d[i * 4 + 1]!, bb = d[i * 4 + 2]!;
          const fr = tr * ta + r * (1 - ta), fg = tg * ta + gg * (1 - ta), fb = tb * ta + bb * (1 - ta);
          const l1 = lum(fr, fg, fb), l2 = lum(r, gg, bb);
          ratios.push((Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05));
        }
        ratios.sort((p, q) => p - q);
        const ratio = ratios[Math.floor(ratios.length * 0.1)]!; // 10th percentile: the bad patches decide
        const large = b.size >= 24 || (b.size >= 18.66 && b.weight >= 700);
        const need = large ? 3 : 4.5;
        if (ratio < need) out.push({ sel: b.sel, text: b.text, ratio: Math.round(ratio * 100) / 100, need, color: b.color, rect: { x: Math.round(b.x), y: Math.round(b.y), w: Math.round(b.w), h: Math.round(b.h) } });
      }
      return out;
    },
    { png, boxes },
  );
}
