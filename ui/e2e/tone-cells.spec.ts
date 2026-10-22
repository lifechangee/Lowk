// Cell -> element mapping of the adaptive overlay (owner photo: a subtitle row was light across its full width although
// only the right part of the backdrop was light). A synthetic 4x3 tone grid with ONLY the left column dark is injected
// through the mock; only the left parts of the scrim / text may darken, whole rows must never take one colour.
import { expect, test } from '@playwright/test';

const DARK = { tone: 'dark', rgb: '8,10,40', fg: '247,247,249', inv: '0,0,0', int: '120,160,255', mv: '255,190,90', luma: null, busy: null, scrim_min: 0 };
const LIGHT = { tone: 'light', rgb: '240,240,235', fg: '12,12,15', inv: '255,255,255', int: '20,60,160', mv: '150,70,0', luma: null, busy: null, scrim_min: 0 };

function grid(isDark: (col: number, row: number) => boolean) {
  const cells = Array.from({ length: 12 }, (_, i) => (isDark(i % 4, Math.floor(i / 4)) ? DARK : LIGHT));
  return { ...LIGHT, grid: { cols: 4, rows: 3, cells } };
}

test.use({ viewport: { width: 600, height: 384 }, locale: 'ru-RU', timezoneId: 'UTC' });

async function open(page: import('@playwright/test').Page, tone: unknown) {
  await page.addInitScript((sw) => ((window as unknown as { __swatch: unknown }).__swatch = sw), { tone, color: '#888' });
  await page.goto('/overlay.html?still=1&bg=swatch&t=60&lang=ru&bare=1');
  await page.waitForSelector('.scrim');
  await page.waitForTimeout(600);
}

// red channel per hard-stop cell; engines serialise `c a% b%` as one stop (webkit) or two (chromium)
const hard = (row: string) => {
  const r = [...row.matchAll(/rgb\((\d+),/g)].map((m) => +m[1]!);
  return r.length === 8 ? r.filter((_, i) => i % 2 === 0) : r;
};
const rgbParts = (s: string) => (s.match(/\d+(\.\d+)?/g) ?? []).slice(0, 3).map(Number);

test('only the LEFT column dark: scrim cell variables map by column and row', async ({ page }) => {
  await open(page, grid((c) => c === 0));
  const vars = await page.evaluate(() => {
    const el = document.querySelector('.scrim') as HTMLElement;
    return Array.from({ length: 12 }, (_, i) => getComputedStyle(el).getPropertyValue(`--c${i}`).trim());
  });
  vars.forEach((v, i) => {
    const dark = i % 4 === 0; // index = row * 4 + col: column 0 of every row
    expect(v, `--c${i}`).toContain(dark ? '8' : '240');
  });
});

test('only the LEFT column dark: painted scrim is dark on the left, light on the right, in every row', async ({ page }) => {
  await open(page, grid((c) => c === 0));
  const box = await page.locator('.frame').boundingBox();
  expect(box).toBeTruthy();
  // hide all content so only the scrim is painted, then sample real pixels
  await page.addStyleTag({ content: '.ov [data-blk], .ov [data-blk] * { visibility: hidden !important }' });
  const buf = await page.screenshot({ clip: box! });
  const px = await page.evaluate(async (b64) => {
    const img = new Image();
    img.src = 'data:image/png;base64,' + b64;
    await img.decode();
    const c = document.createElement('canvas');
    c.width = img.width;
    c.height = img.height;
    const g = c.getContext('2d')!;
    g.drawImage(img, 0, 0);
    const at = (fx: number, fy: number) => Array.from(g.getImageData(Math.floor(fx * (img.width - 1)), Math.floor(fy * (img.height - 1)), 1, 1).data);
    const out: Record<string, number[]> = {};
    for (const [ri, fy] of [[0, 0.12], [1, 0.5], [2, 0.88]] as const) for (const [ci, fx] of [[0, 0.1], [1, 0.37], [2, 0.62], [3, 0.9]] as const) out[`${ri},${ci}`] = at(fx, fy);
    return out;
  }, buf.toString('base64'));
  const lum = (p: number[]) => (p[0]! + p[1]! + p[2]!) / 3;
  for (const r of [0, 1, 2]) {
    const left = lum(px[`${r},0`]!);
    for (const c of [1, 2, 3]) expect(lum(px[`${r},${c}`]!), `row ${r} col ${c} must be lighter than col 0 (${left})`).toBeGreaterThan(left + 60);
  }
});

test('only the LEFT column dark: a long text line is light-on-dark at the left and dark-on-light at the right', async ({ page }) => {
  await open(page, grid((c) => c === 0));
  const g = await page.evaluate(() => {
    const root = document.querySelector('.ov') as HTMLElement;
    const r = root.getBoundingClientRect();
    return Array.from(document.querySelectorAll<HTMLElement>('.ov [data-gt][data-grad]')).map((el) => {
      const b = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return { img: cs.backgroundImage, pos: cs.backgroundPosition, size: cs.backgroundSize, left: b.left - r.left, w: b.width, rootW: r.width, text: (el.textContent ?? '').slice(0, 20) };
    });
  });
  expect(g.length).toBeGreaterThan(0);
  for (const e of g) {
    // each of the three row-gradients lists the dark cell's text colour (247,247,249) first and the light cell's (12,12,15) after it
    const rows = e.img.split(/\), (?=linear-gradient)/);
    expect(rows.length, e.text).toBe(3);
    for (const row of rows) {
      const stops = hard(row);
      expect(stops.length, e.text).toBe(4);
      expect(stops[0], `${e.text}: left stop`).toBe(247);
      for (const s of stops.slice(1)) expect(s, `${e.text}: right stops`).toBe(12);
    }
    // layer width equals the whole overlay, offset by the element's own left: the glyph at x takes the colour of the column at x
    expect(e.size.split(',')[0]).toContain(`${e.rootW}px`);
    const xs = e.pos.split(',')[0]!.trim().split(' ')[0]!;
    expect(Math.abs(parseFloat(xs) + e.left)).toBeLessThan(1.5);
  }
});

test('only the RIGHT column dark: mirrored, and rows are independent', async ({ page }) => {
  await open(page, grid((c, r) => c === 3 && r === 2));
  const rows = await page.evaluate(() => {
    const el = document.querySelector<HTMLElement>('.ov [data-gt][data-grad]')!;
    return getComputedStyle(el).backgroundImage.split(/\), (?=linear-gradient)/);
  });
  const reds = hard;
  expect(reds(rows[0]!)).toEqual([12, 12, 12, 12]);
  expect(reds(rows[1]!)).toEqual([12, 12, 12, 12]);
  expect(reds(rows[2]!)).toEqual([12, 12, 12, 247]);
});
