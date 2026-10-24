// Area selection page (ScreenshotArea): drag / cancel / confirm against a mock Tauri IPC, labels RU + EN.
import { expect, test, type Page } from '@playwright/test';

test.use({ viewport: { width: 800, height: 600 } });

const mock = () => {
  const w = window as any;
  const cbs: Record<number, (e: unknown) => void> = {};
  const handlers: Record<string, number[]> = {};
  let n = 1;
  w.__calls = [];
  w.__TAURI_INTERNALS__ = {
    transformCallback: (cb: (e: unknown) => void) => ((cbs[n] = cb), n++),
    unregisterCallback: (id: number) => delete cbs[id],
    invoke: async (cmd: string, args: any) => {
      if (cmd === 'plugin:event|listen') (handlers[args.event] ??= []).push(args.handler);
      else w.__calls.push({ cmd, args });
      return null;
    },
  };
  w.__fire = (event: string, payload: unknown) => (handlers[event] ?? []).forEach((id) => cbs[id]({ event, id, payload }));
};

async function open(page: Page, lang: 'ru' | 'en', keys = true) {
  await page.addInitScript(mock);
  await page.goto('/area.html');
  await page.waitForFunction(() => (window as any).__fire && document.querySelector('.sel'));
  await page.waitForTimeout(100);
  await page.evaluate(([l, k]) => (window as any).__fire('area://open', { lang: l, keys: k }), [lang, keys] as const);
}
const calls = (page: Page) => page.evaluate(() => (window as any).__calls as Array<{ cmd: string; args: any }>);

for (const lang of ['ru', 'en'] as const) {
  test(`hint and live size label (${lang})`, async ({ page }) => {
    await open(page, lang);
    await expect(page.getByTestId('hint')).toHaveText(lang === 'ru' ? /Esc или правый клик/ : /Esc or right-click/);
    await page.mouse.move(100, 100);
    await page.mouse.down();
    await page.mouse.move(300, 250, { steps: 4 });
    await expect(page.getByTestId('size')).toHaveText('200 × 150');
    const r = await page.getByTestId('rect').boundingBox();
    expect(r).toMatchObject({ x: 100, y: 100, width: 200, height: 150 });
    await page.mouse.up();
  });
  test(`mouse-only hint without Esc (${lang})`, async ({ page }) => {
    await open(page, lang, false);
    await expect(page.getByTestId('hint')).not.toContainText('Esc');
  });
}

test('release confirms exactly the dragged rect, any direction, clamped to the window', async ({ page }) => {
  await open(page, 'en');
  await page.mouse.move(400, 300);
  await page.mouse.down();
  await page.mouse.move(100, 200, { steps: 3 });
  await page.mouse.up();
  expect(await calls(page)).toEqual([{ cmd: 'area_confirm', args: { rect: { x: 100, y: 200, w: 300, h: 100 } } }]);
});

test('Enter confirms while a rect exists; Esc and right-click cancel', async ({ page }) => {
  await open(page, 'en');
  await page.mouse.move(50, 50);
  await page.mouse.down();
  await page.mouse.move(150, 120, { steps: 3 });
  await page.keyboard.press('Enter');
  expect((await calls(page)).map((c) => c.cmd)).toEqual(['area_confirm']);
  await page.evaluate(() => (window as any).__fire('area://open', { lang: 'en', keys: true }));
  await page.keyboard.press('Enter');
  await page.keyboard.press('Escape');
  await page.evaluate(() => (window as any).__fire('area://open', { lang: 'en', keys: true }));
  await page.mouse.click(300, 300, { button: 'right' });
  expect((await calls(page)).map((c) => c.cmd)).toEqual(['area_confirm', 'area_cancel', 'area_cancel']);
});

test('a click without a drag (< 5 px) cancels', async ({ page }) => {
  await open(page, 'en');
  await page.mouse.move(200, 200);
  await page.mouse.down();
  await page.mouse.move(203, 202);
  await page.mouse.up();
  expect(await calls(page)).toEqual([{ cmd: 'area_cancel', args: {} }]);
});

test('Enter with no rect does nothing', async ({ page }) => {
  await open(page, 'en');
  await page.keyboard.press('Enter');
  expect(await calls(page)).toEqual([]);
});
