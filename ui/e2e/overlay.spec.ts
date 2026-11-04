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

test('F8 header has a labelled Stop button that stops with one click', async ({ page }) => {
  await page.goto(url('bg=dark&mode=interactive'));
  const stop = page.getByRole('button', { name: 'Stop' });
  await expect(stop).toBeVisible();
  await stop.click();
  await expect(stop).toHaveCount(0);
  await expect(page.locator('.rec.off')).toBeVisible();
});

test('G2 copies plain answer text and confirms the copy briefly', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: (text: string) => { (window as any).__copied = text; return Promise.resolve(); } } });
  });
  await page.goto(url('bg=dark&mode=interactive'));
  const copy = page.getByTestId('ans-copy');
  await expect(copy).toBeVisible();
  await copy.click();
  await expect(copy).toHaveText('Copied');
  expect(await page.evaluate(() => (window as any).__copied)).not.toContain('**');
  await expect(copy).toHaveText('Copy', { timeout: 2500 });
});

test('G3 Mic and Stop are small icon buttons, labelled by title/aria, light and above the scrim', async ({ page }) => {
  await page.goto('/overlay.html?still=1&t=60&lang=ru&bare=1&mode=interactive');
  const stop = page.getByRole('button', { name: 'Завершить' });
  const mic = page.getByTestId('ov-mic');
  await expect(stop).toBeVisible();
  await expect(mic).toBeVisible();
  await expect(stop).toHaveAttribute('title', 'Завершить');
  expect(await mic.getAttribute('aria-label')).toMatch(/икрофон/);
  await expect(stop.locator('svg')).toHaveCount(1);
  await expect(mic.locator('svg')).toHaveCount(1);
  await expect(stop).toHaveText('');
  await expect(mic).toHaveText('');
  const chipColour = await page.locator('.tpl').evaluate((e) => getComputedStyle(e).color);
  for (const loc of [stop, mic]) {
    const d = await loc.evaluate((e) => { const b = e.getBoundingClientRect(); return { w: b.width, h: b.height, pos: getComputedStyle(e).position, c: getComputedStyle(e).color }; });
    expect(d.w).toBeLessThanOrEqual(28);
    expect(d.h).toBeLessThanOrEqual(28);
    // B10 regression: `all: unset` once dropped `position: relative`, so the buttons painted under the scrim and looked dark
    expect(d.pos).toBe('relative');
    // light: same text colour as the template chip, not the dim secondary tone
    expect(d.c).toBe(chipColour);
  }
});

test('G4 streaming answer can be cancelled and remains visibly marked', async ({ page }) => {
  // still=1: no autoplay script, so Enter is the only thing that starts an answer
  await page.goto('/overlay.html?still=1&bg=dark&t=60&lang=en&bare=1&mode=interactive');
  await page.waitForSelector('.head .tpl');
  // interactive mode focuses the text field, where Enter sends typed text; the stage's bare Enter = ask needs focus elsewhere
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press('Enter');
  const cancel = page.getByTestId('ans-cancel');
  await expect(cancel).toBeVisible();
  await cancel.click();
  await expect(page.getByRole('status')).toHaveText('Answer cancelled');
  await expect(page.getByTestId('ans-copy')).toBeVisible();
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

test('G10 «Мой ответ» renders each backend state with its state-specific copy', async ({ page }) => {
  const cases = [
    ['before_first_ask', 'Мой ответ', 'Начнёт записываться после ⌘↵'],
    ['listening', 'Мой ответ · слушаю 0:12', 'Говорю сейчас'],
    ['paused', 'Мой ответ · пауза', 'Микрофон выключен — ответ не записывается'],
    ['limit', 'Мой ответ · предел', 'Достигнут предел: дальше не записывается'],
    ['closed', 'Мой ответ · сохранён', 'подготовил короткий ответ'],
  ] as const;
  for (const [state, title, body] of cases) {
    await page.goto(`/overlay.html?still=1&t=60&lang=ru&bare=1&my=${state}`);
    const mine = page.getByTestId('my-answer');
    await expect(mine).toContainText(title);
    await expect(mine).toContainText(body);
    if (state === 'listening' || state === 'paused') await expect(page.getByTestId('my-done')).toBeVisible();
    else await expect(page.getByTestId('my-done')).toHaveCount(0);
    if (state === 'listening') await expect(mine.locator('.my-dot')).toBeVisible();
    else await expect(mine.locator('.my-dot')).toHaveCount(0);
    if (state === 'closed') await expect(mine).toHaveClass(/dimmed/);
  }
});

test('G10 interactive «Готово» closes listening and focused Esc closes it without leaving interactive mode', async ({ page }) => {
  await page.goto('/overlay.html?still=1&t=60&lang=ru&bare=1&expose=1&mode=interactive&my=listening');
  await page.getByTestId('my-done').click();
  await expect(page.getByTestId('my-answer')).toContainText('Мой ответ · сохранён');
  await page.evaluate(() => (window as unknown as { __ovStore: { applyQa: (u: unknown) => void } }).__ovStore.applyQa({ qa_id: 'mock-qa', state: 'listening', text: 'ответ', partial_text: '', elapsed_ms: 0, char_count: 6 }));
  const mine = page.getByTestId('my-answer');
  await mine.focus();
  await page.keyboard.press('Escape');
  await expect(mine).toContainText('Мой ответ · сохранён');
  await expect(page.locator('.ov')).toHaveAttribute('data-mode', 'interactive');
});

test('G10 interactive click expands the strip text for scrolling', async ({ page }) => {
  await page.goto('/overlay.html?still=1&t=60&lang=ru&bare=1&mode=interactive&my=closed&myText=Это%20длинный%20ответ%20для%20проверки%20раскрытия%20полного%20текста%20в%20интерактивном%20режиме.%20'.repeat(1));
  const mine = page.getByTestId('my-answer');
  await mine.click();
  await expect(mine).toHaveClass(/expanded/);
  await expect(mine.locator('.tx')).toHaveCSS('overflow-y', 'auto');
});

test('G10 complement refusal explains that there is nothing to add', async ({ page }) => {
  await page.goto('/overlay.html?still=1&t=60&lang=ru&bare=1&emptyComplement=1');
  await expect(page.locator('.ans .body')).toContainText('Сначала скажите ответ — дополнять пока нечего');
  await expect(page.locator('.ans .body .err')).toHaveCount(0);
});

test('G12 slow answer shows live elapsed time and the retry option after twenty seconds', async ({ page }) => {
  await page.clock.install();
  await page.goto('/overlay.html?still=1&t=60&lang=ru&bare=1&slow=1');
  await expect(page.getByTestId('ans-cancel')).toBeVisible();
  await page.clock.runFor(6_000);
  await expect(page.locator('.ans .body')).toContainText('Отвечает дольше обычного… 6 с');
  await expect(page.locator('.ans .body')).not.toContainText('Можно отменить');
  await page.clock.runFor(14_000);
  await expect(page.locator('.ans .body')).toContainText('Отвечает дольше обычного… 20 с');
  await expect(page.locator('.ans .body')).toContainText('Можно отменить и спросить снова');
});
