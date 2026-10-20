// Owner round 2 (docs/M0-FEEDBACK.md G1, G3, G5, G6, G7): behaviour the pixel matrix cannot express.
// Permanent regression tests (the G-ids have no expected-fail stage: they were written with the fixes).
import { expect, test } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

test('G1 main text size: S/M/L/XL presets apply on click; the control never moves or resizes under the pointer', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/appearance');
  await page.waitForSelector('.tb');
  const seg = page.getByRole('radiogroup', { name: 'Interface text size' });
  await expect(seg.getByRole('radio')).toHaveText(['S', 'M', 'L', 'XL']);
  const w0 = (await seg.boundingBox())!.width;
  const want: Record<string, number> = { s: 13 / 14, m: 1, l: 16 / 14, xl: 18 / 14 };
  for (const id of ['xl', 'm', 'l', 's', 'xl']) {
    await seg.locator(`[data-size=${id}]`).click(); // each click lands where the previous layout left the button
    await page.waitForTimeout(80);
    await expect(seg.locator(`[data-size=${id}]`)).toHaveAttribute('aria-checked', 'true');
    expect(Number(await page.evaluate(() => document.getElementById('app')!.style.zoom))).toBeCloseTo(want[id]!, 2);
    expect(Math.abs((await seg.boundingBox())!.width - w0)).toBeLessThan(1);
  }
  const m = await page.evaluate(() => ({ sh: document.documentElement.scrollHeight, ih: innerHeight, sw: document.documentElement.scrollWidth, iw: innerWidth }));
  expect(m.sh).toBeLessThanOrEqual(m.ih);
  expect(m.sw).toBeLessThanOrEqual(m.iw);
});

test('G1 overlay text size: separate S/M/L/XL control with a live preview', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/appearance');
  await page.waitForSelector('.prev');
  const fs = (sel: string) => page.locator(sel).first().evaluate((e) => parseFloat(getComputedStyle(e).fontSize));
  const main0 = await page.evaluate(() => document.getElementById('app')!.style.zoom);
  expect(await fs('.prev .ans')).toBe(14);
  await page.getByRole('radiogroup', { name: 'Overlay text size' }).locator('[data-size=xl]').click();
  await page.waitForTimeout(80);
  expect(await fs('.prev .ans')).toBe(18);
  await page.getByRole('radiogroup', { name: 'Overlay text size' }).locator('[data-size=s]').click();
  await page.waitForTimeout(80);
  expect(await fs('.prev .ans')).toBe(12);
  // the two controls are independent: the main window zoom did not move
  expect(await page.evaluate(() => document.getElementById('app')!.style.zoom)).toBe(main0);
});

test('G5 library home has no header «New call» button (the card is the only one); other screens open a popover', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await page.waitForSelector('.nc');
  await expect(page.locator('.tb button.btn')).toHaveCount(0);
  await expect(page.getByRole('button', { name: /New call/ })).toHaveCount(0);
  await expect(page.locator('.nc h2')).toHaveText('New call');

  await page.evaluate(() => (location.hash = '#/settings/general'));
  const trigger = page.locator('.tb button.btn.primary');
  await expect(trigger).toBeVisible();
  expect(await page.evaluate(() => location.hash)).toBe('#/settings/general');
  await trigger.click();
  const pop = page.getByRole('dialog', { name: 'New call' });
  await expect(pop).toBeVisible();
  expect(await page.evaluate(() => location.hash), 'the button no longer jumps to the home card').toBe('#/settings/general');
  await expect(pop.locator('select')).toHaveCount(1); // folder
  await expect(pop.locator('[data-source-value]')).toHaveCount(1); // source
  await page.keyboard.press('Escape');
  await expect(pop).toHaveCount(0);

  await trigger.click();
  await pop.getByRole('button', { name: /Start recording/ }).click();
  await expect(pop).toHaveCount(0);
  await page.waitForSelector('.recpill');
});

test('G5 on a folder screen the popover preselects that folder', async ({ page }) => {
  await page.goto('/main.html?lang=en#/folder/01JC8Z3K6Q9X2M4N7R5T0V0001');
  await page.waitForSelector('.tb');
  await page.locator('.tb button.btn.primary').click();
  const sel = page.getByRole('dialog', { name: 'New call' }).locator('select');
  expect(await sel.inputValue()).toBe('01JC8Z3K6Q9X2M4N7R5T0V0001');
});

test('G3 source picker: running apps with icon+name, presets, «Other…» from installed apps, no identifier typing', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await page.waitForSelector('.nc');
  const picker = page.locator('.nc [data-source-value]');
  await expect(picker).toHaveAttribute('data-source-value', 'app:us.zoom.xos');
  await picker.getByRole('button').click();
  const list = page.getByRole('listbox');
  await expect(list.getByText('Playing audio now')).toBeVisible();
  const opts = list.getByRole('option');
  // running (producing audio): Zoom + Spotify, with a tile/icon each
  await expect(opts.filter({ hasText: 'Spotify' }).locator('.ic')).toHaveCount(1);
  const win = await page.evaluate(() => /Windows/i.test(navigator.userAgent)); // the suite's Chrome reports a Windows UA; FaceTime is macOS only
  for (const name of ['Google Meet (Chrome)', 'Microsoft Teams', 'Discord', 'Telegram', 'Slack', ...(win ? [] : ['FaceTime'])]) await expect(opts.filter({ hasText: name })).toHaveCount(1);
  if (win) await expect(opts.filter({ hasText: 'FaceTime' })).toHaveCount(0);
  await expect(opts.filter({ hasText: 'Zoom' })).toHaveCount(1); // running entry replaces the preset, no duplicate
  await expect(list.locator('input')).toHaveCount(0);
  await opts.filter({ hasText: 'Discord' }).click();
  await expect(picker).toHaveAttribute('data-source-value', win ? 'app:Discord.exe' : 'app:com.hnc.Discord');
  await expect(picker.getByRole('button')).toContainText('Discord');

  await picker.getByRole('button').click();
  await page.getByRole('option', { name: 'Other…' }).click();
  const dlg = page.getByRole('dialog', { name: 'Choose an app' });
  await expect(dlg.getByRole('button')).not.toHaveCount(0);
  await dlg.getByRole('textbox', { name: 'Search installed apps' }).fill('vlc');
  await dlg.getByRole('button', { name: 'VLC' }).click();
  await expect(dlg).toHaveCount(0);
  await expect(picker).toHaveAttribute('data-source-value', 'app:org.videolan.vlc');
  await expect(picker.getByRole('button')).toContainText('VLC');

  await picker.getByRole('button').click();
  await page.getByRole('option', { name: 'Whole system audio' }).click();
  await expect(picker).toHaveAttribute('data-source-value', 'system');
});

test('G6 overlay footer shows the real binds: hold-click, brief, complement, screenshot', async ({ page }) => {
  await page.setViewportSize({ width: 600, height: 384 });
  await page.goto('/overlay.html?still=1&bg=dark&t=60&lang=en&bare=1');
  await page.waitForSelector('.foot');
  const foot = await page.locator('.foot .hints').first().innerText();
  expect(foot).toMatch(/(⌃|Ctrl)\s*hold: click/);
  expect(foot).toMatch(/(⌘↵|Ctrl\+Enter)\s*brief/);
  expect(foot).toMatch(/(⌘⇧↵|Ctrl\+Shift\+Enter)\s*complement/);
  expect(foot).toMatch(/(⌘⇧S|Ctrl\+Shift\+S)\s*screenshot/);
});

test('G6 holding Ctrl makes the overlay clickable (template menu opens) and releasing returns to click-through', async ({ page }) => {
  await page.setViewportSize({ width: 600, height: 384 });
  await page.goto('/overlay.html?still=1&bg=dark&t=60&lang=en&bare=1');
  await page.waitForSelector('.ov');
  await expect(page.locator('.ov')).not.toHaveClass(/hover/);
  await page.keyboard.down('Control');
  await expect(page.locator('.ov')).toHaveClass(/hover/);
  await expect(page.locator('.head .mode .lbl')).toHaveText('Clickable');
  await page.locator('.tpl').click();
  await expect(page.locator('.menu')).toBeVisible();
  await page.keyboard.up('Control');
  await expect(page.locator('.ov')).not.toHaveClass(/hover/);
  await expect(page.locator('.menu')).toHaveCount(0);
});

test('G7 overlay header shows the screenshot count and thumbnails; the clear button empties the queue', async ({ page }) => {
  await page.setViewportSize({ width: 600, height: 384 });
  await page.goto('/overlay.html?still=1&bg=dark&t=60&lang=en&bare=1');
  await page.waitForSelector('.head .chip');
  const chip = page.locator('.head .chip');
  await expect(chip.locator('img.thumb')).toHaveCount(2);
  await expect(chip.locator('.lbl')).toHaveText('×2');
  await page.keyboard.press('s');
  await expect(chip.locator('.lbl')).toHaveText('×3');
  await chip.getByRole('button', { name: 'Clear screenshots' }).click();
  await expect(page.locator('.head .chip')).toHaveCount(0);
});
