// Microphone on/off (overlay button, main-window control, folder preset, bind) and the adaptive scrim toggle.
import { expect, test } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

const ACME = '01JC8Z3K6Q9X2M4N7R5T0V0001'; // folder fixture «Acme Corp»

async function startCall(page: import('@playwright/test').Page) {
  await page.locator('.side .nc button.btn.primary').click();
  await page.locator('.np button.btn.primary').click();
  await page.waitForSelector('.recpill');
}

async function openLive(page: import('@playwright/test').Page) {
  await page.locator('.recpill').getByRole('button', { name: 'Open' }).click();
  await page.waitForSelector('.livebar');
}

test('overlay: the mic button toggles the microphone while recording', async ({ page }) => {
  await page.goto('/overlay.html?still=1&t=60&lang=en&bare=1&bg=dark');
  const mic = page.getByTestId('ov-mic');
  await expect(mic).toBeVisible();
  await expect(mic).toHaveAttribute('aria-pressed', 'true');
  await expect(mic).toHaveAttribute('aria-label', 'Turn microphone off');
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'false');
  await expect(mic).toHaveAttribute('aria-label', 'Turn microphone on');
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'true');
});

test('overlay (RU): the labels are Russian', async ({ page }) => {
  await page.goto('/overlay.html?still=1&t=60&lang=ru&bare=1&bg=dark');
  await expect(page.getByTestId('ov-mic')).toHaveAttribute('aria-label', 'Выключить микрофон');
});

test('main window: the live bar mic control switches the microphone without stopping the recording', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await startCall(page);
  await openLive(page);
  const mic = page.getByTestId('mic-toggle');
  await expect(mic).toHaveAttribute('aria-pressed', 'true');
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('.livebar')).toContainText('Microphone off');
  await expect(page.locator('.recpill')).toHaveCount(1); // still recording
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'true');
});

test('folder preset «off» starts new calls with the microphone off; «inherit» follows the global setting', async ({ page }) => {
  await page.goto(`/main.html?lang=en#/folder/${ACME}`);
  const sel = page.getByTestId('folder-mic').locator('select');
  await expect(sel).toHaveValue('inherit');
  await sel.selectOption('off');
  await expect(sel).toHaveValue('off');
  await startCall(page);
  await openLive(page);
  await expect(page.getByTestId('mic-toggle')).toHaveAttribute('aria-pressed', 'false');
});

test('global «record my microphone» off + inherit starts muted, and the setting is in General', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/general');
  const tg = page.getByRole('switch', { name: 'Record my microphone' });
  await expect(tg).toHaveAttribute('aria-checked', 'true'); // default: on
  await tg.click();
  await expect(tg).toHaveAttribute('aria-checked', 'false');
  await page.evaluate(() => (location.hash = '#/'));
  await startCall(page);
  await openLive(page);
  await expect(page.getByTestId('mic-toggle')).toHaveAttribute('aria-pressed', 'false');
});

test('the mic bind is editable and unbound by default', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/hotkeys');
  await page.waitForSelector('.be');
  const row = page.locator('.hk', { hasText: 'Microphone on / off' });
  await expect(row.locator('.unset')).toHaveText('Not set');
});

test('adaptive scrim: off by default, hint mentions Screen Recording, toggles on and off', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/appearance');
  const tg = page.getByRole('switch', { name: 'Adaptive scrim colour (for smart glasses)' });
  await expect(tg).toHaveAttribute('aria-checked', 'false');
  await expect(page.getByText('Screen Recording permission')).toBeVisible();
  await tg.click();
  await expect(tg).toHaveAttribute('aria-checked', 'true');
  await tg.click();
  await expect(tg).toHaveAttribute('aria-checked', 'false');
});

test('adaptive scrim (RU) label', async ({ page }) => {
  await page.goto('/main.html?lang=ru#/settings/appearance');
  await expect(page.getByRole('switch', { name: 'Адаптивный цвет подложки (для очков)' })).toHaveAttribute('aria-checked', 'false');
});
