// Settings -> About (mock IPC): version/build/licence/source, the privacy statement, third-party licences and the
// diagnostics export. RU and EN, with the layout check that nothing is truncated or overflowing.
import { expect, test, type Page } from '@playwright/test';
import { lintInPage, type Issue } from './lib/lint';

test.use({ viewport: { width: 1280, height: 800 }, timezoneId: 'UTC' });

for (const lang of ['ru', 'en'] as const) {
  const open = async (page: Page) => {
    await page.goto(`/main.html?lang=${lang}#/settings/about`);
    await expect(page.getByTestId('about-version')).toHaveText('0.0.1');
  };

  test.describe(lang, () => {
    test.use({ locale: lang === 'ru' ? 'ru-RU' : 'en-US' });

    test('shows version, commit and date, the AGPL notice and the source link', async ({ page }) => {
      await open(page);
      await expect(page.getByTestId('about-build')).toHaveText('a1b2c3d4e5f6 · 2026-10-25');
      await expect(page.getByTestId('about-license')).toContainText('Affero');
      await expect(page.getByTestId('about-info')).toContainText('AGPL-3.0-or-later');
      const popup = page.waitForEvent('popup');
      await page.getByRole('button', { name: lang === 'ru' ? 'Открыть репозиторий' : 'Open repository' }).click();
      expect((await popup).url()).toContain('github.com/lowk-app/lowk');
    });

    test('the privacy statement says no telemetry and lists models, cloud providers, local servers and update checks', async ({ page }) => {
      await open(page);
      const box = page.getByTestId('about-privacy');
      await expect(box).toContainText(lang === 'ru' ? 'не собирает телеметрию' : 'collects no telemetry');
      await expect(box.locator('li')).toHaveCount(4);
      await expect(box).toContainText('Hugging Face');
      await expect(box).toContainText('OpenRouter');
      await expect(box).toContainText(lang === 'ru' ? 'обновлений' : 'Update checks');
    });

    test('opens the bundled third-party licences', async ({ page }) => {
      await open(page);
      await page.getByRole('button', { name: lang === 'ru' ? 'Показать лицензии' : 'Show licences' }).click();
      const pre = page.getByTestId('about-licenses');
      await expect(pre).toContainText('Downloadable models');
      await expect(pre).toContainText('OpenMDW-1.1');
      const wide = await pre.evaluate((e) => e.scrollWidth > e.clientWidth + 1);
      expect(wide, 'long URLs wrap instead of scrolling sideways').toBe(false);
      await page.keyboard.press('Escape');
      await expect(pre).toHaveCount(0);
    });

    test('Save diagnostics reports where the zip went and offers reveal-in-folder', async ({ page }) => {
      await open(page);
      await page.getByRole('button', { name: lang === 'ru' ? 'Сохранить диагностику' : 'Save diagnostics' }).click();
      const res = page.getByTestId('diag-result');
      await expect(res).toContainText('lowk-diagnostics.zip');
      await expect(res).toContainText('47 KB');
      await expect(res.getByRole('button', { name: lang === 'ru' ? 'Показать в папке' : 'Show in folder' })).toBeVisible();
    });

    for (const width of [1280, 820]) {
      test(`nothing is truncated or overflowing at ${width}px (with the export result shown)`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        await open(page);
        await page.getByRole('button', { name: lang === 'ru' ? 'Сохранить диагностику' : 'Save diagnostics' }).click();
        await expect(page.getByTestId('diag-result')).toBeVisible();
        const issues = (await page.evaluate(lintInPage, { root: '.set main' })) as Issue[];
        expect(issues.filter((i) => ['overflow', 'outside', 'text-overlap', 'box-overlap', 'flex-gap'].includes(i.rule))).toEqual([]);
        const clipped = await page.evaluate(() =>
          [...document.querySelectorAll('.set main *')]
            .filter((e) => {
              const cs = getComputedStyle(e);
              return cs.textOverflow === 'ellipsis' && e.scrollWidth > e.clientWidth + 1;
            })
            .map((e) => e.textContent?.trim().slice(0, 40)),
        );
        expect(clipped, 'ellipsized text on the About page').toEqual([]);
        const pageScroll = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
        expect(pageScroll).toBe(false);
      });
    }
  });
}
