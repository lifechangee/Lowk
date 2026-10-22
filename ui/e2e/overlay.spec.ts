// Overlay behaviour that the visual matrix cannot express: adaptive grid (F5), text size (F6),
// stop button (F8), duplicate-event guard (F10), legibility over a busy backdrop (F11).
import { expect, test } from '@playwright/test';
import { measureContrast } from './lib/contrast';
import { dupWords } from './lib/lint';

test.use({ viewport: { width: 600, height: 384 }, locale: 'en-US', timezoneId: 'UTC' });

const url = (q: string) => `/overlay.html?still=1&t=60&lang=en&bare=1&${q}`;

test('F5 text blocks pick their colour per region and the scrim is a smooth gradient', async ({ page }) => {
  await page.goto(url('bg=split'));
  await page.waitForSelector('.scrim');
  await page.waitForTimeout(400);
  const fg = await page.evaluate(() => [...document.querySelectorAll<HTMLElement>('.ov [data-blk]')].map((e) => ({ cls: e.className.split(' ')[0], fg: e.style.getPropertyValue('--fg') })));
  // Every block got its own computed fg from the grid (not left to the global tone).
  expect(fg.length).toBeGreaterThanOrEqual(8);
  for (const b of fg) expect(b.fg, b.cls).toMatch(/^\d+,\d+,\d+$/);
  // Rows 0/1 are dark-left + light-right = mixed; the bottom row is `color` (white text).
  const bottom = fg.find((b) => b.cls === 'foot')!;
  expect(bottom.fg).toBe('255,255,255');
  // Smoothness: the scrim exposes 12 cell colours, rendered as gradients rather than hard tiles.
  const bg = await page.evaluate(() => getComputedStyle(document.querySelector('.scrim i')!).backgroundImage);
  expect(bg).toContain('linear-gradient');
});

test('F5 without a grid the global tone applies (no per-block overrides)', async ({ page }) => {
  await page.goto(url('bg=split&grid=0'));
  await page.waitForSelector('.scrim');
  await page.waitForTimeout(300);
  const overrides = await page.evaluate(() => [...document.querySelectorAll<HTMLElement>('.ov [data-blk]')].filter((e) => e.style.getPropertyValue('--fg')).length);
  expect(overrides).toBe(0);
});

test('F5 text over every region of the split backdrop is legible', async ({ page }) => {
  await page.goto(url('bg=split'));
  await page.waitForSelector('.scrim');
  await page.waitForTimeout(400);
  const c = await measureContrast(page, '.frame', ['.bar']);
  const bad = c.filter((x) => x.ratio < 4.5).map((x) => `${x.sel} "${x.text}" ${x.ratio}:1`);
  expect(bad).toEqual([]);
});

test('F6 text size setting changes the answer, subtitle and «Мой ответ» text live', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/overlay.html?still=1&bg=dark&t=60&lang=en');
  await page.waitForSelector('.ans .body');
  const size = (sel: string) => page.locator(sel).first().evaluate((e) => parseFloat(getComputedStyle(e).fontSize));
  const before = { ans: await size('.ans .body'), my: await size('.my .tx'), sub: await size('.sub .txt') };
  await page.locator('.bar select').nth(3).selectOption('15');
  await page.waitForTimeout(100);
  const after = { ans: await size('.ans .body'), my: await size('.my .tx'), sub: await size('.sub .txt') };
  expect(after.ans).toBeGreaterThan(before.ans);
  expect(after.my).toBeGreaterThan(before.my);
  expect(after.sub).toBeGreaterThan(before.sub);
});

test('F8 header has a Stop recording button that stops with one click', async ({ page }) => {
  await page.goto(url('bg=dark&mode=interactive'));
  const stop = page.getByRole('button', { name: 'Stop recording' });
  await expect(stop).toBeVisible();
  await stop.click();
  await expect(stop).toHaveCount(0);
  await expect(page.locator('.rec.off')).toBeVisible();
});

test('F10 each streamed delta is applied exactly once even when every event arrives twice', async ({ page }) => {
  await page.goto('/overlay.html?bg=dark&t=60&lang=en&bare=1&dup=1');
  await page.waitForSelector('.ans .body');
  await page.getByText('Press').first().waitFor({ state: 'detached', timeout: 20000 }).catch(() => {});
  await page.keyboard.press('Enter');
  // The demo runs its own scripted answers; wait for the first one to finish and compare.
  const seen: string[] = [];
  for (let i = 0; i < 80; i++) {
    await page.waitForTimeout(100);
    const t = await page.locator('.ans .body').first().innerText().catch(() => '');
    if (t) seen.push(t);
  }
  expect(seen.flatMap((t) => dupWords(t))).toEqual([]);
  const last = seen.at(-1) ?? '';
  expect(last.length).toBeGreaterThan(20);
});

test('F11 text keeps AA contrast over a black/white checkerboard at opacity 0, 60 and 90', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const bad: string[] = [];
  for (const bg of ['dark', 'light'] as const) {
    for (const t of [0, 60, 90]) {
      await page.goto(`/overlay.html?still=1&bg=${bg}&t=${t}&lang=en`);
      await page.waitForSelector('.frame');
      await page.addStyleTag({ content: '.stage{background:repeating-conic-gradient(#fff 0 25%,#000 0 50%) 0 0/16px 16px !important}.code,.doc,.slide{display:none !important}' });
      await page.waitForTimeout(250);
      const c = await measureContrast(page, '.frame', ['.bar']);
      for (const x of c.filter((x) => ['div.body', 'div.txt', 'b', 'span.tx'].includes(x.sel) && x.ratio < 4.5)) bad.push(`${bg} t${t} ${x.sel} "${x.text}" ${x.ratio}:1`);
    }
  }
  expect(bad).toEqual([]);
});
