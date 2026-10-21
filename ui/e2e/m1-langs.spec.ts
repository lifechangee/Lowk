// Per-call languages (BRIEF «Языки созвона»), the install card's real download total and template binds.
// Mock IPC (the real backend: crates/lowk-session tests + langpolicy tests + `just qa-bundle`).
import { expect, test, type Page } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

const started = (page: Page) => page.evaluate(() => (globalThis as { __lowkStartOpts?: { languages?: string[] | null } }).__lowkStartOpts?.languages);
const chip = (page: Page, code: string) => page.locator(`[data-testid=lang-picker] [data-lang=${code}]`).first();

test('new call card: ru+en by default, picks are sent with the call and remembered for the next one', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await expect(chip(page, 'ru')).toHaveAttribute('aria-pressed', 'true');
  await expect(chip(page, 'en')).toHaveAttribute('aria-pressed', 'true');
  await expect(chip(page, 'pl')).toHaveAttribute('aria-pressed', 'false');
  await chip(page, 'pl').click();
  await chip(page, 'ru').click();
  await expect(chip(page, 'ru')).toHaveAttribute('aria-pressed', 'false');
  await page.getByRole('button', { name: /Start recording/ }).first().click();
  await expect.poll(() => started(page)).toEqual(['en', 'pl']);
  // stop; the selector keeps the last choice (not the settings default)
  await page.locator('section').getByRole('button', { name: 'Stop', exact: true }).click();
  await expect(chip(page, 'pl')).toHaveAttribute('aria-pressed', 'true');
  await expect(chip(page, 'ru')).toHaveAttribute('aria-pressed', 'false');
});

test('at least one language stays selected; «More…» lists every language the engine supports', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await chip(page, 'ru').click();
  await chip(page, 'en').click();
  await expect(chip(page, 'en')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('status')).toContainText('at least one language');
  await expect(page.locator('[data-testid=lang-picker] [data-lang=vi]')).toHaveCount(0);
  await page.getByRole('button', { name: 'More…' }).first().click();
  await expect(chip(page, 'vi')).toBeVisible();
  await expect(chip(page, 'zh')).toBeVisible();
});

test('settings: the default languages persist and are what the card starts with', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/general');
  const g = page.locator('[data-testid=lang-picker]').first();
  await expect(g.locator('[data-lang=ru]')).toHaveAttribute('aria-pressed', 'true');
  await g.locator('[data-lang=de]').click();
  await g.locator('[data-lang=ru]').click();
  await page.evaluate(() => (location.hash = '#/'));
  await expect(chip(page, 'de')).toHaveAttribute('aria-pressed', 'true');
  await expect(chip(page, 'ru')).toHaveAttribute('aria-pressed', 'false');
});

test('header popover has the same language selector', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/hotkeys');
  await page.locator('.tb button.btn.primary').click();
  const np = page.locator('.np');
  await expect(np.locator('[data-lang=ru]')).toHaveAttribute('aria-pressed', 'true');
  await np.locator('[data-lang=en]').click();
  await np.getByRole('button', { name: /Start recording/ }).click();
  await expect.poll(() => started(page)).toEqual(['ru']);
});

test('russian UI: language names and labels are translated', async ({ page }) => {
  await page.goto('/main.html?lang=ru#/');
  await expect(chip(page, 'ru')).toHaveText('Русский');
  await expect(chip(page, 'en')).toHaveText('Английский');
  await expect(page.locator('.nc')).toContainText('Языки');
});

test('onboarding capture step offers the languages, pre-filled with system language + English', async ({ page }) => {
  await page.goto('/main.html?lang=en&fresh#/onboarding/2');
  await expect(page.getByText('Call languages')).toBeVisible();
  await expect(chip(page, 'en')).toHaveAttribute('aria-pressed', 'true');
  await expect(chip(page, 'ru')).toHaveAttribute('aria-pressed', 'false');
});

test('engine install card shows the real total (live model + Whisper correction)', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/providers');
  const card = page.getByTestId('engine-card');
  await expect(card).toBeVisible();
  await expect(card.getByTestId('engine-total')).toContainText('1.4 GB');
  await expect(card.getByTestId('engine-total')).toContainText('Whisper');
  await expect(card.getByRole('button', { name: /Download \(1\.4 GB\)/ })).toBeVisible();
});

test('hotkeys: every template, including a user-created one, can get its own key; the same key twice warns', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/hotkeys');
  const list = page.getByTestId('template-binds');
  const row = list.locator('[data-slot="ask_template:objection"]');
  await expect(row).toContainText('Ask: Client objection');
  await expect(list.locator('[data-slot=ask_say]')).toBeVisible();
  await expect(list.locator('[data-slot=ask_recap]')).toBeVisible();
  await row.getByRole('button', { name: /Client objection/ }).click();
  await page.keyboard.press('Meta+KeyJ');
  await expect(row.locator('kbd').last()).toHaveText('J');
  // the same key on another template is flagged as a duplicate
  const say = list.locator('[data-slot=ask_say]');
  await say.getByRole('button', { name: /what to say/i }).click();
  await page.keyboard.press('Meta+KeyJ');
  await expect(say).toContainText('Already assigned');
  // a different key resolves it; the first bind stays
  await say.getByRole('button', { name: /what to say/i }).click();
  await page.keyboard.press('Meta+KeyK');
  await expect(say).not.toContainText('Already assigned');
  await expect(row.locator('kbd').last()).toHaveText('J');
});
