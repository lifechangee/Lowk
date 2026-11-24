import { expect, test, type Page } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import type { OverlayStore } from '../src/overlay/store.svelte';
import type { SourceRef } from '../src/lib/ipc/bindings';

test.use({ viewport: { width: 600, height: 384 }, locale: 'en-US', timezoneId: 'UTC' });
const LONG = Array.from({ length: 36 }, (_, i) => `${i + 1}. Пункт ${i + 1}: подробное объяснение ответа с основными цифрами и выводами для разговора.`).join('\n');
const doc: SourceRef = { kind: 'doc', id: 'doc-1', folder: { kind: 'unsorted' }, title: '01_SHPARGALKA_zakrepit', at_s: null, page: null, heading_path: [], chunk_id: null, pinned: false };
const sources: SourceRef[] = [{ ...doc, kind: 'instructions', id: 'folder-1', title: 'Инструкции папки' }, ...Array.from({ length: 5 }, (_, i) => ({ ...doc, chunk_id: `chunk-${i}` }))];

async function start(page: Page, px: number, refs = sources) {
  await page.goto('/overlay.html?still=1&expose=1&t=60&lang=ru&bare=1&bg=dark&mode=interactive&scenario=assist&my=listening');
  await page.waitForSelector('.ans .body');
  await page.evaluate(({ px, refs, text }) => {
    const s = (window as unknown as { __ovStore: OverlayStore }).__ovStore;
    s.fontPx = px;
    s.applyLlm({ type: 'start', req_id: 'b19', qa_id: 'q-b19' });
    s.applyLlm({ type: 'delta', req_id: 'b19', text, seq: 0 });
    s.applyLlm({ type: 'sources', req_id: 'b19', sources: refs, withheld: 0 });
  }, { px, refs, text: LONG });
  await expect(page.getByTestId('ans-cancel')).toBeVisible();
}

async function finish(page: Page) {
  await page.evaluate(() => (window as unknown as { __ovStore: OverlayStore }).__ovStore.applyLlm({ type: 'done', req_id: 'b19', truncated: false, usage: null }));
  await expect(page.getByTestId('ans-cancel')).toHaveCount(0);
}

// 14px = OverlaySettings::default(); 18px = OVERLAY_STEPS XL in settings/AppearancePage.
for (const px of [14, 18]) {
  test(`B19 one scrollable message preserves reading space and position (${px}px)`, async ({ page }, info) => {
    await start(page, px);
    await expect(page.getByTestId('ans-copy')).toHaveCount(0);
    await expect(page.locator('.ans .srcs')).toHaveCount(0);
    const body = page.getByTestId('ans-body');
    // The reader has scrolled back to the first line while tokens are still arriving.
    await body.evaluate((e) => { e.scrollTop = 0; });
    await expect.poll(() => body.evaluate((e) => e.scrollTop)).toBe(0);
    // Let the scroll event pause follow before the terminal event adds the footer.
    await page.waitForTimeout(100);
    await finish(page);
    await expect.poll(() => body.evaluate((e) => e.scrollTop)).toBe(0);
    const geometry = await body.evaluate((e) => {
      const text = e.querySelector('.bt')!;
      const range = document.createRange();
      range.selectNodeContents(text);
      const first = range.getClientRects()[0]!;
      const b = e.getBoundingClientRect();
      return { height: e.clientHeight, line: parseFloat(getComputedStyle(e).lineHeight), firstTop: first.top, firstBottom: first.bottom, top: b.top, bottom: b.bottom };
    });
    expect(geometry.height).toBeGreaterThanOrEqual(3 * geometry.line);
    expect(geometry.firstTop).toBeGreaterThanOrEqual(geometry.top - 1);
    expect(geometry.firstTop - geometry.top).toBeLessThan(geometry.line);
    expect(geometry.firstBottom).toBeLessThanOrEqual(geometry.bottom);
    const done = page.getByTestId('my-done');
    await expect(done).toHaveText('Готово');
    await expect(done).toBeInViewport({ ratio: 1 });
    await expect(page.locator('.inp input')).toBeInViewport({ ratio: 1 });

    const dir = process.env.B19_SCREENSHOT_DIR ?? info.outputPath('screenshots');
    await mkdir(dir, { recursive: true });
    await page.screenshot({ path: join(dir, `b19-${info.project.name}-${px}px-top.png`) });
    await body.evaluate((e) => { e.scrollTop = e.scrollHeight; });
    const copy = body.getByTestId('ans-copy');
    await expect(copy).toHaveText('Копировать');
    await expect(copy).toBeInViewport({ ratio: 1 });
    const chips = body.locator('.srcs button');
    await expect(chips).toHaveText(['Инструкции папки', '01_SHPARGALKA_zakrepit']);
    for (const chip of await chips.all()) await expect(chip).toBeInViewport({ ratio: 1 });
    await expect(done).toBeInViewport({ ratio: 1 });
    await expect(page.locator('.inp input')).toBeInViewport({ ratio: 1 });
    await page.screenshot({ path: join(dir, `b19-${info.project.name}-${px}px-end.png`) });
  });
}

test('B19 pinned follow stops at the text end when the footer appears', async ({ page }) => {
  await start(page, 18);
  const body = page.getByTestId('ans-body');
  const top = await body.evaluate((e) => e.scrollTop);
  expect(top).toBeGreaterThan(100);
  await finish(page);
  // Removing Cancel enlarges the viewport: follow may move up to keep the text end aligned.
  await expect.poll(() => body.evaluate((e) => e.scrollTop)).toBeLessThanOrEqual(top);
  await expect.poll(() => body.evaluate((e) => Math.abs(e.querySelector('.bt')!.getBoundingClientRect().bottom - e.getBoundingClientRect().bottom))).toBeLessThanOrEqual(1);
});

test('B19 sources wrap beyond two rows and all footer notes scroll with the text', async ({ page }) => {
  await start(page, 14, Array.from({ length: 12 }, (_, i) => ({ ...doc, id: `doc-${i}`, title: `Документ ${i + 1} с длинным названием` })));
  await page.evaluate(() => {
    const s = (window as unknown as { __ovStore: OverlayStore }).__ovStore;
    s.imagesSent = 2;
    s.withheld = 3;
  });
  await expect(page.getByTestId('ans-images')).toHaveCount(0);
  await finish(page);
  const body = page.getByTestId('ans-body');
  await expect(body.getByTestId('ans-images')).toContainText('2');
  const chips = body.locator('.srcs').last();
  await expect(chips.locator('button')).toHaveCount(12);
  expect(await chips.evaluate((e) => e.clientHeight)).toBeGreaterThan(3 * 24);
  await body.evaluate((e) => { e.scrollTop = e.scrollHeight; });
  await expect(chips.locator('button').last()).toBeInViewport({ ratio: 1 });
  await expect(chips.locator('.held')).toContainText('3');
  await expect(chips.locator('.held')).toBeInViewport({ ratio: 1 });
});
