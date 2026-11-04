import { expect, test } from '@playwright/test';

for (const lang of ['ru', 'en'] as const) {
  test(`scenario start and overlay switching (${lang})`, async ({ page }) => {
    await page.goto(`/main.html?lang=${lang}#/`);
    await page.locator('.side .nc button.btn.primary').click();
    const start = page.locator('.np button.btn.primary');
    await expect(start).toBeDisabled();
    await page.getByTestId('start-assist').click();
    await expect(start).toBeEnabled();
    await start.click();
    await expect(page.locator('.recpill')).toBeVisible();

    await page.goto(`/overlay.html?still=1&bare=1&mode=interactive&scenario=null&lang=${lang}`);
    const prompt = page.getByTestId('scenario-prompt');
    await expect(prompt).toBeVisible();
    await prompt.getByRole('button', { name: lang === 'ru' ? 'Помощь' : 'Help' }).click();
    await expect(prompt).toHaveCount(0);
    await expect(page.getByTestId('my-answer')).toBeVisible();
    const chip = page.getByTestId('scenario-chip');
    await chip.click();
    await page.getByRole('menuitem', { name: lang === 'ru' ? 'Веду встречу' : 'Leading a meeting' }).click();
    await expect(page.getByTestId('my-answer')).toHaveCount(0);
    await expect(chip).toHaveText(lang === 'ru' ? 'Веду встречу' : 'Leading a meeting');
    await chip.click();
    await page.getByRole('menuitem', { name: lang === 'ru' ? 'Слушаю' : 'Listening' }).click();
    await expect(page.getByTestId('my-answer')).toHaveCount(0);
  });
}
