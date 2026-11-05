import { expect, test } from '@playwright/test';

const CALL = '01JC8Z3K6Q9X2M4N7R5T0V0101';

test('a screenshot loads and its preview closes with Escape', async ({ page }) => {
  await page.goto(`/main.html?lang=en#/call/${CALL}`);
  await page.locator('[data-tab="shots"]').click();
  const thumbnail = page.locator('.shots2 button.im').first();
  const image = thumbnail.locator('img');
  await expect(image).toBeVisible();
  await expect(image).toHaveAttribute('src', /^data:image\/png;base64,/);
  await expect.poll(() => image.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
  await thumbnail.click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
});
