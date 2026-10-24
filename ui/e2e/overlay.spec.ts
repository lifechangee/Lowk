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

// Header layout: nothing in the 600 px header may be truncated or clipped (the mic toggle once pushed the
// template name, the mode label and the «×2» queue chip into ellipses).
for (const lang of ['ru', 'en'] as const) {
  for (const px of [13, 15]) {
    test(`header: nothing is truncated or clipped (${lang}, ${px}px, all states)`, async ({ page }) => {
      await page.goto(`/overlay.html?still=1&t=60&lang=${lang}&bare=1&bg=dark&expose=1`);
      await page.waitForSelector('.head .tpl');
      await page.evaluate((n) => {
        (window as unknown as { __ovStore: { fontPx: number } }).__ovStore.fontPx = n;
      }, px);
      const states: Array<[string, string]> = [
        ['passive', `s.setMode('passive'); s.menuOpen = false; s.phase = 'recording'; s.setQueue(2, s.thumbs)`],
        ['interactive', `s.setMode('interactive'); s.menuOpen = false`],
        ['menu open', `s.setMode('interactive'); s.menuOpen = true`],
        ['moving', `s.menuOpen = false; s.setMode('moving')`],
        ['hover', `s.setMode('hover')`],
        ['no queue', `s.setMode('passive'); s.setQueue(0)`],
        ['queue 12', `s.setQueue(12, [])`],
        ['paused', `s.setQueue(2, []); s.phase = 'paused'`],
        ['idle', `s.phase = 'idle'`],
        ['mic muted', `s.phase = 'recording'; s.micOn = false`],
      ];
      for (const [name, js] of states) {
        await page.evaluate((code) => {
          const s = (window as unknown as { __ovStore: unknown }).__ovStore;
          new Function('s', code)(s);
        }, js);
        await page.waitForTimeout(150);
        const bad = await page.evaluate(() => {
          const out: string[] = [];
          const head = document.querySelector<HTMLElement>('.head')!;
          const hr = head.getBoundingClientRect();
          const ov = document.querySelector<HTMLElement>('.ov')!.getBoundingClientRect();
          if (head.scrollWidth > head.clientWidth) out.push(`.head scrollWidth ${head.scrollWidth} > ${head.clientWidth}`);
          const all = [...head.querySelectorAll<HTMLElement>('*')].filter((e) => !e.closest('.sr') && !e.classList.contains('sr') && !(e instanceof SVGElement));
          for (const e of all) {
            const r = e.getBoundingClientRect();
            if (!r.width && !r.height) continue;
            const id = `${e.tagName.toLowerCase()}.${e.className && typeof e.className === 'string' ? e.className : ''}«${(e.textContent ?? '').trim().slice(0, 20)}»`;
            if (!e.querySelector('.menu') && e.scrollWidth > e.clientWidth + 0.5) out.push(`${id} scrollWidth ${e.scrollWidth} > clientWidth ${e.clientWidth}`);
            if (e.closest('.menu')) {
              if (r.right > ov.right + 0.5 || r.left < ov.left - 0.5) out.push(`${id} outside overlay`);
            } else if (r.right > hr.right - 0.5 + 16 || r.left < hr.left - 0.5) out.push(`${id} outside header`);
            if (getComputedStyle(e).textOverflow === 'ellipsis' && e.scrollWidth > e.clientWidth) out.push(`${id} ellipsised`);
          }
          return out;
        });
        expect(bad, `${lang} ${px}px ${name}`).toEqual([]);
      }
    });
  }
}
