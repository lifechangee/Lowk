// B8: the «overlay visible on screen recording» switch (Settings) and its reminders (overlay badge, pill dot).
import { expect, test } from '@playwright/test';
import { lintInPage, type Issue } from './lib/lint';

const ov = (lang: string, extra = '') => `/overlay.html?still=1&t=60&lang=${lang}&bare=1&${extra}`;

test.describe('settings switch', () => {
  test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

  test('General has the switch (off by default, with the explanation) and it saves', async ({ page }) => {
    await page.goto('/main.html?lang=en#/settings/general');
    const sw = page.getByRole('switch', { name: 'Show the overlay in screen recordings' });
    await expect(sw).toBeVisible();
    await expect(sw).toHaveAttribute('aria-checked', 'false');
    await expect(page.getByText('Turn on to make the overlay visible in screenshots and screen sharing. It is hidden by default.')).toBeVisible();
    await sw.click();
    await expect(sw).toHaveAttribute('aria-checked', 'true');
    await sw.click();
    await expect(sw).toHaveAttribute('aria-checked', 'false');
  });

  test('Russian wording as decided', async ({ page }) => {
    await page.goto('/main.html?lang=ru#/settings/general');
    await expect(page.getByRole('switch', { name: 'Показывать оверлей на записи экрана' })).toBeVisible();
    await expect(page.getByText('Включите, чтобы оверлей был виден в скриншотах и при показе экрана. По умолчанию он скрыт.')).toBeVisible();
  });

  test('Hotkeys page lists the unbound «Overlay visibility for capture» action', async ({ page }) => {
    await page.goto('/main.html?lang=en#/settings/hotkeys');
    const row = page.getByText('Overlay visibility for capture', { exact: true });
    await expect(row).toBeVisible();
  });
});

test.describe('overlay reminders', () => {
  test.use({ viewport: { width: 600, height: 384 }, locale: 'en-US', timezoneId: 'UTC' });

  test('no badge by default; amber badge when visible, in the header, overlapping no control', async ({ page }) => {
    await page.goto(ov('en'));
    await page.waitForSelector('.head');
    await expect(page.getByTestId('ov-capture-badge')).toHaveCount(0);
    await page.goto(ov('en', 'capvis=1'));
    const b = page.getByTestId('ov-capture-badge');
    await expect(b).toHaveText('Visible on recording');
    // amber, not red
    const bg = await b.evaluate((e) => getComputedStyle(e).borderTopColor);
    const [r, g, bl] = bg.match(/\d+/g)!.map(Number);
    expect(r).toBeGreaterThan(200);
    expect(g).toBeGreaterThan(120);
    expect(g).toBeLessThan(200);
    expect(bl).toBeLessThan(80);
    // inside the header, not overlapping any other header control
    const boxes = await page.evaluate(() => {
      const head = document.querySelector('.head')!.getBoundingClientRect();
      const bd = document.querySelector('[data-testid=ov-capture-badge]')!.getBoundingClientRect();
      const others = [...document.querySelectorAll('.head > *')].filter((e) => !(e as HTMLElement).dataset.testid && !e.classList.contains('sp')).map((e) => e.getBoundingClientRect());
      const hit = others.filter((o) => o.width > 0 && !(bd.right <= o.left || bd.left >= o.right || bd.bottom <= o.top || bd.top >= o.bottom)).length;
      return { inHead: bd.left >= head.left && bd.right <= head.right && bd.top >= head.top && bd.bottom <= head.bottom, hit };
    });
    expect(boxes).toEqual({ inHead: true, hit: 0 });
    const issues = ((await page.evaluate(lintInPage, { root: '.head' })) as Issue[]).filter((i) => i.rule !== 'grid');
    expect(issues).toEqual([]);
  });

  test('Russian badge text', async ({ page }) => {
    await page.goto(ov('ru', 'capvis=1'));
    await expect(page.getByTestId('ov-capture-badge')).toHaveText('Виден на записи');
  });

  test('the badge stays with the stop button, mic and shots chip present, and in interactive mode', async ({ page }) => {
    await page.goto(ov('en', 'capvis=1&mode=interactive'));
    await expect(page.getByTestId('ov-capture-badge')).toBeVisible();
    await expect(page.getByTestId('ov-mic')).toBeVisible();
    const issues = ((await page.evaluate(lintInPage, { root: '.head' })) as Issue[]).filter((i) => i.rule !== 'grid');
    expect(issues).toEqual([]);
  });

  test('pill: amber dot only when visible, inside the strip and clear of the dot / label (vertical and horizontal)', async ({ page }) => {
    for (const edge of ['right', 'top'] as const) {
      const [w, h] = edge === 'right' ? [22, 88] : [140, 22];
      await page.setViewportSize({ width: w, height: h });
      await page.goto(ov('en', 'view=pill&expose=1'));
      await page.waitForSelector('.pill');
      await page.evaluate((e) => ((window as unknown as { __ovStore: { pillEdge: string } }).__ovStore.pillEdge = e), edge);
      await expect(page.getByTestId('pill-capture-dot')).toHaveCount(0);
      await page.goto(ov('en', 'view=pill&capvis=1&expose=1'));
      await page.waitForSelector('.pill');
      await page.evaluate((e) => ((window as unknown as { __ovStore: { pillEdge: string } }).__ovStore.pillEdge = e), edge);
      await expect(page.getByTestId('pill-capture-dot')).toBeVisible();
      const r = await page.evaluate(() => {
        const rc = (sel: string) => document.querySelector(sel)?.getBoundingClientRect();
        const d = rc('[data-testid=pill-capture-dot]')!;
        const others = ['.pill .lbl', '.pill i'].map(rc).filter((x): x is DOMRect => !!x);
        const apart = others.every((o) => d.left >= o.right || d.right <= o.left || d.top >= o.bottom || d.bottom <= o.top);
        return { in: d.left >= 0 && d.right <= innerWidth && d.top >= 0 && d.bottom <= innerHeight, apart, vertical: document.querySelector('.pill')!.classList.contains('v') };
      });
      expect(r, edge).toEqual({ in: true, apart: true, vertical: edge === 'right' });
    }
  });
});
