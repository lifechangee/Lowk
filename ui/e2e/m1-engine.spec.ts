// M1 QA: the speech-model download card (Settings > Providers). Before this the install commands had
// no UI at all and the record bind silently played the scripted demo.
import { expect, test } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

async function openCard(page: import('@playwright/test').Page, query = '') {
  await page.goto(`/main.html?lang=en${query}#/settings/providers`);
  const card = page.getByTestId('engine-card');
  await expect(card).toBeVisible();
  return card;
}

test('engine card: not installed says recording cannot start, installs with progress, ends installed', async ({ page }) => {
  const card = await openCard(page);
  await expect(card).toContainText('Not installed');
  await expect(card).toContainText('cannot start until the model is installed');
  await card.getByRole('button', { name: /Download/ }).click();
  await expect(card.getByRole('progressbar')).toBeVisible();
  await expect(card.getByRole('button', { name: 'Cancel' })).toBeVisible();
  await expect(card).toContainText('Installed', { timeout: 5000 });
  await expect(card).toContainText('transcribed on this computer');
});

test('engine card: a failed download (checksum) shows a readable reason and a retry button', async ({ page }) => {
  const card = await openCard(page, '&install=fail');
  await card.getByRole('button', { name: /Download/ }).click();
  const alert = card.getByRole('alert');
  await expect(alert).toContainText('integrity check', { timeout: 5000 });
  await expect(card.getByRole('button', { name: /Try again/ })).toBeVisible();
  await expect(card).toContainText('Not installed');
});

test('engine card: cancel returns to the install button', async ({ page }) => {
  const card = await openCard(page);
  await card.getByRole('button', { name: /Download/ }).click();
  await card.getByRole('button', { name: 'Cancel' }).click();
  await expect(card.getByRole('button', { name: /Download|Try again/ })).toBeVisible({ timeout: 5000 });
});

test('engine card: model installed but the speech component missing says so (no silent demo)', async ({ page }) => {
  const card = await openCard(page, '&runtime=missing');
  await card.getByRole('button', { name: /Download/ }).click();
  await expect(card).toContainText('Installed', { timeout: 5000 });
  await expect(card.getByTestId('engine-runtime-missing')).toContainText('speech component');
  await expect(card).not.toContainText('transcribed on this computer');
});
