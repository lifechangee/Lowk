// Auto-update UI against the mock IPC (`?update=` picks the scenario, see MockBackend): the notice in the
// main window, the Settings section, progress / error states, RU + EN, and the truncation lint.
import { expect, test, type Page } from '@playwright/test';
import { lintInPage, type Issue } from './lib/lint';

test.use({ viewport: { width: 1280, height: 800 }, timezoneId: 'UTC' });

const T = {
  en: { found: 'Version 0.2.0 is available', update: 'Update', later: 'Later', skip: 'Skip this version', unavailable: 'Auto-update is not available in this build', checkNow: 'Check now', latest: 'You are on the latest version', restart: 'Restart', waiting: 'Restarts right after the recording', net: 'Could not reach the update server', sig: 'The update signature did not verify', rec: 'A recording is running', auto: 'Check for updates automatically' },
  ru: { found: 'Доступна версия 0.2.0', update: 'Обновить', later: 'Позже', skip: 'Пропустить версию', unavailable: 'Автообновление недоступно в этой сборке', checkNow: 'Проверить сейчас', latest: 'У вас последняя версия', restart: 'Перезапустить', waiting: 'Перезапуск сразу после записи', net: 'Не удалось связаться с сервером обновлений', sig: 'Подпись обновления не прошла проверку', rec: 'Идёт запись', auto: 'Проверять обновления автоматически' },
} as const;

const open = (page: Page, lang: 'en' | 'ru', update: string, hash = '#/') =>
  page.goto(`/main.html?lang=${lang}${update ? `&update=${update}` : ''}${hash}`);

for (const lang of ['en', 'ru'] as const) {
  const t = T[lang];
  test.describe(lang, () => {
    test('without a signing key: no notice, the settings say auto-update is unavailable', async ({ page }) => {
      await open(page, lang, '');
      await expect(page.getByTestId('update-notice')).toHaveCount(0);
      await open(page, lang, '', '#/settings/general');
      await expect(page.getByTestId('update-unavailable')).toHaveText(t.unavailable);
      await expect(page.getByRole('button', { name: t.checkNow })).toHaveCount(0);
      await expect(page.getByTestId('update-version')).toHaveText('0.1.0');
    });

    test('notice: version + notes, update downloads with progress and offers a restart', async ({ page }) => {
      await open(page, lang, 'available');
      const n = page.getByTestId('update-notice');
      await expect(n).toContainText(t.found);
      await expect(page.getByTestId('update-notes')).toContainText('Faster start of the speech engine.');
      await expect(n.getByRole('button', { name: t.later })).toBeVisible();
      await expect(n.getByRole('button', { name: t.skip })).toBeVisible();
      await n.getByRole('button', { name: t.update, exact: true }).click();
      await expect(n).toHaveAttribute('data-phase', 'downloading');
      await expect(n.getByRole('progressbar')).toBeVisible();
      await expect(n).toHaveAttribute('data-phase', 'ready', { timeout: 8000 });
      await n.getByRole('button', { name: t.restart, exact: true }).click();
      await expect(n).toHaveCount(0);
    });

    test('notice: Later hides it, Skip version forgets it', async ({ page }) => {
      await open(page, lang, 'available');
      const n = page.getByTestId('update-notice');
      await n.getByRole('button', { name: t.later }).click();
      await expect(n).toHaveCount(0);
      await page.reload(); // «Позже» lasts until the next start
      await expect(n).toBeVisible();
      await n.getByRole('button', { name: t.skip }).click();
      await expect(page.getByTestId('update-notice')).toHaveCount(0);
    });

    test('a bad signature shows a clear message with no retry and nothing to restart', async ({ page }) => {
      await open(page, lang, 'badsig');
      const n = page.getByTestId('update-notice');
      await n.getByRole('button', { name: t.update, exact: true }).click();
      await expect(page.getByTestId('update-error')).toContainText(t.sig, { timeout: 8000 });
      await expect(n.getByRole('button', { name: t.restart })).toHaveCount(0);
    });

    test('settings: check now, last check time, toggles; offline shows a network error', async ({ page }) => {
      await open(page, lang, 'idle', '#/settings/general');
      await expect(page.getByText(t.auto)).toBeVisible();
      const toggle = page.getByRole('switch', { name: t.auto });
      await expect(toggle).toBeChecked();
      await toggle.click();
      await expect(toggle).not.toBeChecked();
      await page.getByRole('button', { name: t.checkNow }).click();
      await expect(page.getByTestId('update-note')).toHaveText(t.latest);

      await open(page, lang, 'offline', '#/settings/general');
      await page.getByRole('button', { name: t.checkNow }).click();
      await expect(page.getByTestId('update-note')).toContainText(t.net);
    });

    test('settings: a check during a recording is refused with an explanation', async ({ page }) => {
      await open(page, lang, 'recording', '#/settings/general');
      await page.getByRole('button', { name: t.checkNow }).click();
      await expect(page.getByTestId('update-note')).toContainText(t.rec);
    });

    test('a restart requested during a recording waits for it to end', async ({ page }) => {
      await open(page, lang, 'recording');
      const n = page.getByTestId('update-notice');
      await n.getByRole('button', { name: t.update, exact: true }).click();
      await expect(n).toHaveAttribute('data-phase', 'ready', { timeout: 8000 });
      await n.getByRole('button', { exact: true, name: /^(Restart|Перезапустить)( after recording| после записи)?$/ }).click();
      await expect(page.getByTestId('update-waiting')).toHaveText(t.waiting);
    });

    for (const width of [800, 1280]) {
      test(`nothing is truncated or clipped (${width}px, all states)`, async ({ page }) => {
        await page.setViewportSize({ width, height: 800 });
        const check = async (root: string) => {
          const issues = (await page.evaluate(lintInPage, { root })) as Issue[];
          expect(issues.filter((i) => i.rule === 'overflow' || i.rule === 'outside' || i.rule === 'text-overlap')).toEqual([]);
          expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
        };
        await open(page, lang, 'available');
        await expect(page.getByTestId('update-notice')).toBeVisible();
        await check('[data-testid=update-notice]');
        await page.getByTestId('update-notice').getByRole('button', { name: t.update, exact: true }).click();
        await expect(page.getByTestId('update-notice')).toHaveAttribute('data-phase', 'ready', { timeout: 8000 });
        await check('[data-testid=update-notice]');
        await open(page, lang, 'badsig');
        await page.getByTestId('update-notice').getByRole('button', { name: t.update, exact: true }).click();
        await expect(page.getByTestId('update-error')).toBeVisible({ timeout: 8000 });
        await check('[data-testid=update-notice]');
        await open(page, lang, 'offline', '#/settings/general');
        await page.getByRole('button', { name: t.checkNow }).click();
        await expect(page.getByTestId('update-note')).toBeVisible();
        await check('main, .set, body');
      });
    }
  });
}
