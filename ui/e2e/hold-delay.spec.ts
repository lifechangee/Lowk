import { expect, test } from '@playwright/test';

test('hotkeys: the hold delay for single keys is a select of fixed steps, 0.25 s by default, and the choice is saved to the settings', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/hotkeys');
  const row = page.getByTestId('hold-delay');
  await expect(row).toContainText('Delay for single keys');
  await expect(row).toContainText('How long to hold Ctrl or Alt');
  const sel = row.getByRole('combobox', { name: 'Delay for single keys' });
  await expect(sel).toHaveValue('250');
  await expect(sel.locator('option')).toHaveText(['At once', '0.25 s', '0.5 s', '1 s', '1.5 s', '2 s']);
  await sel.selectOption('0');
  await expect(sel).toHaveValue('0');
  // the choice went to the settings, not only to the control
  await page.goto('/main.html?lang=en#/settings/general');
  await page.goto('/main.html?lang=en#/settings/hotkeys');
  await expect(page.getByTestId('hold-delay').getByRole('combobox')).toHaveValue('0');
});

test('hotkeys: the hold delay reads in Russian with a decimal comma', async ({ page }) => {
  await page.goto('/main.html?lang=ru#/settings/hotkeys');
  const row = page.getByTestId('hold-delay');
  await expect(row).toContainText('Задержка для одиночных клавиш');
  await expect(row).toContainText('Сколько держать Ctrl или Alt, прежде чем сработает действие.');
  await expect(row.locator('option')).toHaveText(['Сразу', '0,25 с', '0,5 с', '1 с', '1,5 с', '2 с']);
});
