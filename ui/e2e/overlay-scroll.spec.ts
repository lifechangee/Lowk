// O6 (subtitles stop advancing) and O7 (long AI answers cut off): the newest text is always visible, the history
// and the answer scroll only while the overlay is clickable, auto-follow resumes, bounds stay strict.
import { expect, test, type Page } from '@playwright/test';

test.use({ viewport: { width: 600, height: 384 }, locale: 'en-US', timezoneId: 'UTC' });

const url = (q = '') => `/overlay.html?still=1&expose=1&t=60&lang=ru&bare=1&bg=dark${q}`;

/** Playwright-WebKit drops the first wheel(s) after the pointer enters a scroller: re-send until the scroller moved. */
async function wheel(page: Page, sel: string, dy: number) {
  const box = (await page.locator(sel).boundingBox())!;
  const pos = () => page.locator(sel).evaluate((e) => e.scrollTop);
  const before = await pos();
  for (let k = 0; k < 4; k++) {
    await page.mouse.move(box.x + 40 + k, box.y + 10);
    await page.waitForTimeout(100);
    await page.mouse.wheel(0, dy);
    await page.waitForTimeout(300);
    if ((await pos()) !== before) return;
  }
}

/** Replays a call of `minutes` minutes (one utterance per ~4 s of call time) through the real store. Returns the last final. */
async function replay(page: Page, from: number, to: number): Promise<string> {
  return page.evaluate(
    async ([a, b]) => {
      const s = (window as unknown as { __ovStore: { applyStt(e: unknown): void } }).__ovStore;
      const tick = () => new Promise((r) => setTimeout(r, 0));
      const words = 'мы обсуждаем квартальный отчёт и план на следующий месяц по бюджету'.split(' ');
      let last = '';
      for (let i = a!; i < b!; i++) {
        // Engine switch / restart halfway: utterance numbers start over (they must NOT be dropped).
        const utt = i < 80 ? i + 1 : i - 79;
        const stream = i % 5 === 4 ? 'me' : 'system';
        const long = i % 7 === 3;
        const n = long ? 40 : 8 + (i % 6);
        const text = `Реплика ${i + 1} ` + Array.from({ length: n }, (_, k) => words[(i + k) % words.length]).join(' ');
        for (const cut of [Math.ceil(text.length / 3), Math.ceil((text.length * 2) / 3)]) {
          s.applyStt({ stream, utt, rev: 0, kind: 'partial', text: text.slice(0, cut), lang: null, t_start_ns: 0, t_audio_end_ns: 0, t_emit_ns: 0 });
          await tick();
        }
        s.applyStt({ stream, utt, rev: 1, kind: 'final', text, lang: null, t_start_ns: 0, t_audio_end_ns: 0, t_emit_ns: 0 });
        await tick();
        last = text;
      }
      return last;
    },
    [from, to],
  );
}

/** The newest subtitle line's last row lies inside the subtitle viewport, and the viewport stays inside the 56px band. */
async function lastVisible(page: Page, text: string) {
  const r = await page.evaluate(() => {
    const hist = document.querySelector('.sub .hist')!.getBoundingClientRect();
    const sub = document.querySelector('.sub')!.getBoundingClientRect();
    const lines = [...document.querySelectorAll<HTMLElement>('.sub .ln')];
    const ln = lines[lines.length - 1]!;
    const b = ln.getBoundingClientRect();
    return { text: ln.innerText, lnBottom: b.bottom, histTop: hist.top, histBottom: hist.bottom, subH: sub.height, n: lines.length };
  });
  expect(r.text).toContain(text.slice(-30));
  expect(r.lnBottom).toBeLessThanOrEqual(r.histBottom + 1.5);
  expect(r.lnBottom).toBeGreaterThan(r.histTop + 4);
  expect(r.subH).toBe(56);
  expect(r.n).toBeLessThanOrEqual(60);
}

test('O6 a long call: the newest line is visible after 3 and after 10 minutes (passive overlay)', async ({ page }) => {
  await page.goto(url());
  await page.waitForSelector('.sub .hist');
  const at3 = await replay(page, 0, 45);
  await lastVisible(page, at3);
  const at10 = await replay(page, 45, 150);
  await lastVisible(page, at10);
  expect(at10).toContain('Реплика 150');
});

test('O6 «Я» lines follow too, and an engine restart (ids start over) does not freeze the subtitles', async ({ page }) => {
  await page.goto(url());
  await page.waitForSelector('.sub .hist');
  await replay(page, 70, 95); // crosses the id reset at 80
  const last = await replay(page, 95, 100); // i=99 -> stream me
  await lastVisible(page, last);
  await expect(page.locator('.sub .ln').last().locator('.spk')).toHaveText('Я');
});

test('O6 history scrolls only while clickable; «новое ↓» shows; auto-follow resumes after idle', async ({ page }) => {
  await page.goto(url('&mode=hover'));
  await page.waitForSelector('.sub .hist');
  await replay(page, 0, 30);
  const hist = page.locator('.sub .hist');
  const box = (await hist.boundingBox())!;
  await page.mouse.move(box.x + 40, box.y + 10);
  await wheel(page, '.sub .hist', -400);
  await expect(page.locator('.sub .more')).toBeVisible();
  const top = await hist.evaluate((e) => e.scrollTop);
  const last = await replay(page, 30, 34); // new speech while scrolled up: the view stays put
  expect(await hist.evaluate((e) => e.scrollTop)).toBeLessThanOrEqual(top + 2);
  await page.waitForTimeout(4600); // idle: back to the newest line
  await expect(page.locator('.sub .more')).toHaveCount(0);
  await lastVisible(page, last);
});

test('O6 passive overlay does not scroll by wheel (click-through, overflow hidden)', async ({ page }) => {
  await page.goto(url());
  await page.waitForSelector('.sub .hist');
  await replay(page, 0, 30);
  expect(await page.locator('.sub .hist').evaluate((e) => getComputedStyle(e).overflowY)).toBe('hidden');
});

const LONG = Array.from({ length: 36 }, (_, i) => `${i + 1}. **Пункт ${i + 1}**: довольно длинное объяснение, которое занимает несколько строк в окне ответа.`).join('\n');

async function stream(page: Page, from: number, to: number) {
  await page.evaluate(
    async ([a, b, full]) => {
      const s = (window as unknown as { __ovStore: { applyLlm(e: unknown): void } }).__ovStore;
      const lines = (full as string).split('\n');
      if (a === 0) s.applyLlm({ type: 'start', req_id: 'long', qa_id: 'q' });
      for (let i = a as number; i < (b as number); i++) {
        s.applyLlm({ type: 'delta', req_id: 'long', seq: i, text: lines[i] + (i < lines.length - 1 ? '\n' : '') });
        await new Promise((r) => setTimeout(r, 0));
      }
      if (b === lines.length) s.applyLlm({ type: 'done', req_id: 'long', truncated: false, usage: null });
    },
    [from, to, LONG] as const,
  );
}

const ansState = (page: Page) =>
  page.evaluate(() => {
    const b = document.querySelector<HTMLElement>('.ans .body')!;
    const ans = document.querySelector('.ans')!.getBoundingClientRect();
    const ov = document.querySelector('.ov')!.getBoundingClientRect();
    return { top: b.scrollTop, max: b.scrollHeight - b.clientHeight, ch: b.clientHeight, ansH: ans.height, ansBottom: ans.bottom, ovBottom: ov.bottom, text: b.innerText };
  });

test('O7 a long streamed answer follows the stream and stays inside strict bounds', async ({ page }) => {
  await page.goto(url());
  await page.waitForSelector('.ans .body');
  const h0 = (await ansState(page)).ansH;
  await stream(page, 0, 36);
  const s = await ansState(page);
  expect(s.max).toBeGreaterThan(200); // far more text than fits
  expect(s.top).toBeGreaterThanOrEqual(s.max - 4); // newest text visible
  expect(s.text).toContain('Пункт 36');
  expect(Math.abs(s.ansH - h0)).toBeLessThanOrEqual(2); // the area did not grow with the text
  expect(s.ansBottom).toBeLessThanOrEqual(s.ovBottom + 1);
  await expect(page.locator('.ans .more')).toHaveCount(0);
});

test('O7 scroll up (hold ⌃): stays put while streaming, «ещё ↓» appears, click or scrolling down resumes', async ({ page }) => {
  await page.goto(url('&mode=hover'));
  await page.waitForSelector('.ans .body');
  await stream(page, 0, 20);
  const body = page.locator('.ans .body');
  const box = (await body.boundingBox())!;
  await page.mouse.move(box.x + 60, box.y + 30);
  await wheel(page, '.ans .body', -300);
  const more = page.locator('.ans .more');
  await expect(more).toBeVisible();
  await expect(more).toHaveText('ещё ↓');
  const top = (await ansState(page)).top;
  await stream(page, 20, 30);
  const mid = await ansState(page);
  expect(mid.top).toBeLessThanOrEqual(top + 2); // user's position kept
  expect(mid.max).toBeGreaterThan(top + 50);
  await more.click();
  await stream(page, 30, 36);
  const end = await ansState(page);
  expect(end.top).toBeGreaterThanOrEqual(end.max - 4);
  await expect(more).toHaveCount(0);
  // scrolling back down by hand also re-pins
  await wheel(page, '.ans .body', -300);
  await expect(more).toBeVisible();
  await wheel(page, '.ans .body', 5000);
  await expect(more).toHaveCount(0);
});

/** Ctrl held = the click-hold bind: wheel events carry ctrlKey (browser zoom gesture). They must scroll, not zoom. */
async function ctrlWheel(page: Page, sel: string, dy: number, mode = 0) {
  return page.locator(sel).evaluate(
    (el, [d, m]) => {
      const r = el.getBoundingClientRect();
      const ev = new WheelEvent('wheel', { deltaY: d, deltaMode: m, ctrlKey: true, bubbles: true, cancelable: true, clientX: r.x + 40, clientY: r.y + 10 });
      el.dispatchEvent(ev);
      return { prevented: ev.defaultPrevented, scrollTop: el.scrollTop, zoom: window.visualViewport?.scale ?? 1, dpr: window.devicePixelRatio };
    },
    [dy, mode] as const,
  );
}

test('scroll: Ctrl+wheel over the AI answer in passive+hold scrolls it and is not a zoom', async ({ page }) => {
  await page.goto(url('&mode=hover'));
  await page.waitForSelector('.ans .body');
  await stream(page, 0, 36);
  const before = (await ansState(page)).top;
  const r = await ctrlWheel(page, '.ans .body', -150);
  expect(r.prevented).toBe(true);
  expect(r.scrollTop).toBeLessThan(before);
  expect(r.zoom).toBe(1);
  const r2 = await ctrlWheel(page, '.ans .body', 3, 1); // line mode
  expect(r2.scrollTop).toBeGreaterThan(r.scrollTop);
});

test('scroll: Ctrl+wheel over the subtitles scrolls the history and is not a zoom', async ({ page }) => {
  await page.goto(url('&mode=hover'));
  await page.waitForSelector('.sub .hist');
  await replay(page, 0, 30);
  const before = await page.locator('.sub .hist').evaluate((e) => e.scrollTop);
  const r = await ctrlWheel(page, '.sub .hist', -120);
  expect(r.prevented).toBe(true);
  expect(r.scrollTop).toBeLessThan(before);
  expect(r.zoom).toBe(1);
});
