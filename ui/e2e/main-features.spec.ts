// Behaviour tests for the main-window features that the browser harness can drive:
// F6 (interface text size), F7 (binds), F8 (stop -> library home), F9 (drag & drop / «Переместить в…»).
import { expect, test, type Page } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

const QUICK = 'Быстрый созвон с подрядчиком'; // an Unsorted call in the mock fixtures

async function drag(page: Page, from: ReturnType<Page['locator']>, to: ReturnType<Page['locator']>, hold = false) {
  const a = (await from.boundingBox())!;
  const b = (await to.boundingBox())!;
  await page.mouse.move(a.x + 40, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(a.x + 60, a.y + a.height / 2 + 12, { steps: 4 });
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 8 });
  if (!hold) await page.mouse.up();
}

test('F6 interface text size scales the main window live', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/appearance');
  await page.waitForSelector('.tb');
  const before = (await page.locator('.tb').boundingBox())!.height;
  await page.getByRole('radiogroup', { name: 'Interface text size' }).locator('[data-size=xl]').click(); // G1: presets
  await page.waitForTimeout(150);
  const after = (await page.locator('.tb').boundingBox())!.height;
  expect(after / before).toBeGreaterThan(1.25);
  const zoom = await page.evaluate(() => document.getElementById('app')!.style.zoom);
  expect(Number(zoom)).toBeCloseTo(18 / 14, 2);
  // the window layout still fills the viewport exactly: no page scroll, nothing hangs below.
  const m = await page.evaluate(() => ({ sh: document.documentElement.scrollHeight, ih: innerHeight, sw: document.documentElement.scrollWidth, iw: innerWidth }));
  expect(m.sh).toBeLessThanOrEqual(m.ih);
  expect(m.sw).toBeLessThanOrEqual(m.iw);
  const app = (await page.locator('.app').boundingBox())!;
  expect(Math.round(app.height)).toBe(800);
});

test('F7 panic is unbound by default, show/hide overlay is on the backslash key', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/hotkeys');
  await page.waitForSelector('.be');
  const panic = page.locator('.hk', { hasText: 'Panic' });
  await expect(panic.locator('.unset')).toHaveText('Not set');
  const overlay = page.locator('.hk', { hasText: 'Show / hide overlay' });
  await expect(overlay.locator('kbd').last()).toHaveText('\\');
});

test('F8 stopping a recording opens library home with the new call on top of Recent', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await page.locator('.side .nc button.btn.primary').click();
  await page.locator('.np button.btn.primary').click();
  await page.waitForSelector('.recpill');
  await page.evaluate(() => (location.hash = '#/folder/sys:unsorted'));
  await page.waitForSelector('.tree');
  await page.locator('.recpill button.stop').click();
  await page.waitForFunction(() => location.hash === '#/' || location.hash === '');
  await page.waitForSelector('.tbl .tr');
  const first = page.locator('.tbl .tr').first();
  await expect(first).toContainText('Новый созвон');
  await expect(first).toHaveClass(/fresh/);
  await expect(page.locator('.recpill')).toHaveCount(0);
});

test('F9 drag a call from Unsorted onto a folder in the workspace tree', async ({ page }) => {
  await page.goto('/main.html?lang=en#/folder/sys:unsorted');
  await page.waitForSelector('.tbl .tr');
  const rows = page.locator('.tbl .tr');
  await expect(rows).toHaveCount(2);
  await drag(page, rows.filter({ hasText: QUICK }), page.locator('.tree .f[data-drop]', { hasText: 'Nova Bank' }), true);
  await expect(page.locator('.tree .f.dropping')).toHaveCount(1); // clear drop highlight
  await page.mouse.up();
  await expect(rows).toHaveCount(1);
  await expect(page.locator('.toast')).toContainText('Moved to “Nova Bank”');
  await expect(page.locator('.tree .f.cur, .tree .f', { hasText: 'Nova Bank' }).first().locator('.n')).toHaveText('3');
});

test('F9 drag a call from Recent onto a folder card on the home screen', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await page.waitForSelector('.tbl .tr');
  const unsorted = page.locator('.tree .f', { hasText: 'Unsorted' }).locator('.n');
  await expect(unsorted).toHaveText('2');
  await page.locator('.view').evaluate((el) => (el.scrollTop = 160)); // Recent and the folder grid in view together
  await drag(page, page.locator('.tbl .tr', { hasText: QUICK }), page.locator('.fc[data-drop]', { hasText: 'Acme Corp' }), true);
  await expect(page.locator('.fc.dropping')).toHaveCount(1);
  await page.mouse.up();
  await expect(unsorted).toHaveText('1');
  await expect(page.locator('.tbl .tr', { hasText: QUICK })).toContainText('Acme Corp');
  expect(await page.evaluate(() => location.hash)).toBe('#/'); // the drop did not open the call
});

test('F9 dragging near the bottom edge scrolls the library so far-away folders stay reachable', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await page.waitForSelector('.tbl .tr');
  const view = page.locator('.view');
  expect(await view.evaluate((el) => el.scrollTop)).toBe(0);
  const row = page.locator('.tbl .tr', { hasText: QUICK });
  await view.evaluate((el) => (el.scrollTop = 120));
  const before = await view.evaluate((el) => el.scrollTop);
  const a = (await row.boundingBox())!;
  await page.mouse.move(a.x + 40, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(a.x + 60, 790, { steps: 6 });
  await page.waitForTimeout(400);
  await page.mouse.move(a.x + 60, 12, { steps: 4 });
  await page.waitForTimeout(600);
  expect(await view.evaluate((el) => el.scrollTop)).toBeLessThan(before);
  await page.keyboard.press('Escape');
  await page.mouse.up();
  await expect(page.locator('.tree .f', { hasText: 'Unsorted' }).locator('.n')).toHaveText('2');
});

test('F9 dropping on the call’s own folder or empty space moves nothing', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await page.waitForSelector('.tbl .tr');
  await drag(page, page.locator('.tbl .tr', { hasText: QUICK }), page.locator('h1'), true);
  await expect(page.locator('.dropping')).toHaveCount(0);
  await page.mouse.up();
  await expect(page.locator('.tree .f', { hasText: 'Unsorted' }).locator('.n')).toHaveText('2');
});

test('F9 «Move to…» context menu works with the mouse and the keyboard', async ({ page }) => {
  await page.goto('/main.html?lang=en#/folder/sys:unsorted');
  await page.waitForSelector('.tbl .tr');
  const rows = page.locator('.tbl .tr');
  await rows.filter({ hasText: QUICK }).click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Move to…' });
  await expect(menu).toBeVisible();
  await menu.getByRole('menuitem', { name: 'Nova Bank' }).click();
  await expect(rows).toHaveCount(1);
  // keyboard: focus the ⋯ button, Enter, arrow, Enter
  await rows.first().getByRole('button', { name: 'Call actions' }).focus();
  await page.keyboard.press('Enter');
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem').first()).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(rows).toHaveCount(0);
});
