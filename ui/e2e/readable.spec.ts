import { expect, test } from '@playwright/test';

const call = '01JC8Z3K6Q9X2M4N7R5T0V0101';

test('readable text can be generated and refreshed while the transcript stays available', async ({ page }) => {
  await page.goto(`/main.html?lang=ru#/call/${call}`);
  await page.getByRole('tab', { name: 'Читаемый текст' }).click();
  const panel = page.getByTestId('readable-tab');
  await expect(panel.getByRole('button', { name: 'Подготовить' })).toBeVisible();
  await panel.getByRole('button', { name: 'Подготовить' }).click();
  await expect(panel.getByRole('button', { name: 'Обновить' })).toBeVisible();
  await panel.getByRole('button', { name: 'Обновить' }).click();
  await expect(panel.getByRole('button', { name: 'Обновить' })).toBeVisible();
  await page.getByRole('tab').first().click();
  await expect(page.getByTestId('readable-tab')).toHaveCount(0);
});

test('generation and errors are visible', async ({ page }) => {
  await page.goto(`/main.html?lang=ru&readable=hold#/call/${call}`);
  await page.getByRole('tab', { name: 'Читаемый текст' }).click();
  await page.getByRole('button', { name: 'Подготовить' }).click();
  await expect(page.getByTestId('readable-tab').getByRole('status')).toContainText('Обрабатываю… часть 3 из 15');
  await page.getByTestId('readable-tab').getByRole('button', { name: 'Остановить' }).click();
  await expect(page.getByTestId('readable-tab').getByRole('button', { name: 'Подготовить' })).toBeVisible();
  await page.goto(`/main.html?lang=ru&readable=error#/call/${call}`);
  await page.getByRole('tab', { name: 'Читаемый текст' }).click();
  await page.getByRole('button', { name: 'Подготовить' }).click();
  await expect(page.getByTestId('readable-tab').getByRole('alert')).toBeVisible();
});

test('local-only calls without a local model cannot be sent to the cloud', async ({ page }) => {
  await page.goto(`/main.html?lang=ru&readable=localonly#/call/${call}`);
  await page.getByRole('tab', { name: 'Читаемый текст' }).click();
  await expect(page.getByTestId('readable-tab').getByRole('button', { name: 'Подготовить' })).toBeDisabled();
});
