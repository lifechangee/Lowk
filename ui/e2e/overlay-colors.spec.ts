// O4: the colour base of the adaptive overlay. (1) The swatch sheet generated from the real AdaptiveTone
// (docs/design/overlay-color-swatches.html) lists every backdrop as verified. (2) The REAL overlay component, fed
// the tone the Rust mapping produced for a backdrop, renders legible text over it (pixel-based contrast).
// SWATCH_PNG=1 also renders the sheet to docs/design/overlay-color-swatches.png.
import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { measureContrast } from './lib/contrast';

const docs = fileURLToPath(new URL('../../docs/design/', import.meta.url));
interface Block { scrim: string; fg: string; int: string; mv: string; body_wcag: number; body_lc: number; muted_wcag: number }
interface Swatch { name: string; mean: string; kind: string; ok: boolean; min_body_wcag: number; min_body_lc: number; min_muted_wcag: number; min_accent_wcag: number; blocks: Block[] }
const data: Swatch[] = JSON.parse(readFileSync(resolve(docs, 'overlay-color-swatches.json'), 'utf8'));

const rgbOf = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(',');

test.describe('swatch sheet', () => {
  test.use({ viewport: { width: 1400, height: 900 } });
  test('lists ~40 backdrops, every one verified, and matches the JSON', async ({ page }, info) => {
    await page.goto(pathToFileURL(resolve(docs, 'overlay-color-swatches.html')).href);
    const cards = page.locator('.card');
    expect(await cards.count()).toBeGreaterThanOrEqual(40);
    expect(await cards.count()).toBe(data.length);
    expect(await page.locator('.card[data-ok="1"]').count()).toBe(data.length);
    for (const s of data) {
      expect(s.min_body_wcag, s.name).toBeGreaterThanOrEqual(4.5);
      expect(s.min_body_lc, s.name).toBeGreaterThanOrEqual(60);
      expect(s.min_muted_wcag, s.name).toBeGreaterThanOrEqual(3);
      expect(s.min_accent_wcag, s.name).toBeGreaterThanOrEqual(3);
    }
    // the first row of every card paints the JSON's scrim and text colours
    const painted = await page.$$eval('.card', (els) => els.map((c) => { const r = c.querySelector('.row') as HTMLElement; const cs = getComputedStyle(r); return { bg: cs.backgroundColor, fg: cs.color }; }));
    painted.forEach((p, i) => {
      const b = data[i]!.blocks[0]!;
      expect(p.bg.replace(/rgba?\(|\)| /g, '').split(',').slice(0, 3).join(','), data[i]!.name).toBe(rgbOf(b.scrim));
      expect(p.fg.replace(/rgba?\(|\)| /g, '').split(',').slice(0, 3).join(','), data[i]!.name).toBe(rgbOf(b.fg));
    });
    if (info.project.name === 'chromium' && process.env.SWATCH_PNG) await page.screenshot({ path: resolve(docs, 'overlay-color-swatches.png'), fullPage: true });
    await expect(page).toHaveScreenshot('swatch-sheet.png', { fullPage: true, maxDiffPixelRatio: 0.02 });
  });
});

const REAL = ['Тёмно-синий (navy)', 'Глубокий синий', 'Королевский синий', 'Чистый синий', 'Красный', 'Фиолетовый', 'Бирюзовый (teal)', 'Зелёный', 'Оранжевый', 'Жёлтый', 'Светло-голубой', 'Средне-серый', 'Оливковый', 'Малиновый'];

test.describe('real overlay over the mapped tones', () => {
  test.use({ viewport: { width: 600, height: 384 }, locale: 'ru-RU', timezoneId: 'UTC' });
  for (const name of REAL) {
    test(`legible over ${name}`, async ({ page }) => {
      const s = data.find((x) => x.name === name)!;
      expect(s, name).toBeTruthy();
      const cell = (b: Block) => ({ tone: s.kind, rgb: rgbOf(b.scrim), fg: rgbOf(b.fg), inv: rgbOf(b.fg) === '247,247,249' ? '0,0,0' : '255,255,255', int: b.int, mv: b.mv, luma: null, busy: null, scrim_min: 0 });
      const cells = Array.from({ length: 96 }, (_, i) => cell(s.blocks[Math.floor((Math.floor(i / 12) * 3) / 8)]!));
      const tone = { ...cell(s.blocks[1]!), grid: { cols: 12, rows: 8, cells } };
      await page.addInitScript((sw) => ((window as unknown as { __swatch: unknown }).__swatch = sw), { tone, color: s.mean });
      await page.goto('/overlay.html?still=1&bg=swatch&t=60&lang=ru&bare=1');
      await page.waitForSelector('.scrim');
      await page.waitForTimeout(500);
      const bad = (await measureContrast(page, '.frame', ['.bar'])).filter((x) => x.ratio < 4.5).map((x) => `${x.sel} "${x.text}" ${x.ratio}:1`);
      expect(bad, name).toEqual([]);
      // the scrim keeps the backdrop's hue: for a navy / blue backdrop it is blue, never the black preset
      const scrim = await page.evaluate(() => getComputedStyle(document.querySelector('.scrim i:nth-child(4)')!).backgroundImage);
      expect(scrim).not.toContain('rgb(12, 12, 15)');
      await expect(page.locator('.frame')).toHaveScreenshot(`swatch-${REAL.indexOf(name)}.png`, { maxDiffPixelRatio: 0.02 });
    });
  }
});
