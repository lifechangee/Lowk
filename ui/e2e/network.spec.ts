// Settings -> Network (mock IPC): the system proxy line, the manual override, the connection check, RU and EN,
// with the layout check that nothing is truncated or overflowing.
import { expect, test, type Page } from '@playwright/test';
import { lintInPage, type Issue } from './lib/lint';

test.use({ viewport: { width: 1280, height: 800 }, timezoneId: 'UTC' });

for (const lang of ['ru', 'en'] as const) {
  const T = (ru: string, en: string) => (lang === 'ru' ? ru : en);
  const open = async (page: Page, q = '') => {
    await page.goto(`/main.html?lang=${lang}${q}#/settings/network`);
    await expect(page.getByTestId('net-system')).not.toHaveText('…');
  };

  test.describe(lang, () => {
    test.use({ locale: lang === 'ru' ? 'ru-RU' : 'en-US' });

    test('shows the system proxy and where it came from', async ({ page }) => {
      await open(page, '&os=windows');
      await expect(page.getByTestId('net-system')).toHaveText(
        T('http://127.0.0.1:10809 (из настроек Windows)', 'http://127.0.0.1:10809 (from Windows settings)'),
      );
    });

    test('says «not used» when there is no system proxy', async ({ page }) => {
      await open(page, '&os=macos');
      await expect(page.getByTestId('net-system')).toHaveText(T('не используется', 'not used'));
    });

    test('the manual proxy is validated, saved, and checked through the typed value', async ({ page }) => {
      await open(page, '&os=windows');
      const field = page.getByRole('textbox', { name: T('Прокси вручную', 'Proxy manually') });
      const save = page.getByRole('button', { name: T('Сохранить', 'Save') });
      await field.fill('ftp://x:1');
      await expect(page.getByTestId('net-bad')).toBeVisible();
      await expect(save).toBeDisabled();
      await field.fill('socks5://127.0.0.1:10808');
      await expect(page.getByTestId('net-bad')).toHaveCount(0);
      await page.getByRole('button', { name: T('Проверить подключение', 'Check connection') }).click();
      const res = page.getByTestId('net-result');
      await expect(res).toContainText('HTTP 200');
      await expect(res).toContainText('312');
      await expect(res).toContainText('socks5://127.0.0.1:10808');
      await save.click();
      await expect(page.getByRole('button', { name: T('Сохранено', 'Saved') })).toBeDisabled();
    });

    test('a failed check names the proxy and the reason; a 403 suggests a VPN or proxy', async ({ page }) => {
      await open(page, '&os=windows');
      await page.getByRole('textbox', { name: T('Прокси вручную', 'Proxy manually') }).fill('http://127.0.0.1:1');
      await page.getByRole('button', { name: T('Проверить подключение', 'Check connection') }).click();
      await expect(page.getByTestId('net-result')).toContainText('Connection refused');
      await page.goto(`/main.html?lang=${lang}&os=windows&check=403#/settings/network`);
      await page.getByRole('button', { name: T('Проверить подключение', 'Check connection') }).click();
      await expect(page.getByTestId('net-result')).toContainText(T('укажите прокси', 'set a proxy'));
    });

    for (const width of [1280, 820]) {
      test(`nothing is truncated or overflowing at ${width}px`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        await open(page, '&os=windows&check=403');
        await page.getByRole('button', { name: T('Проверить подключение', 'Check connection') }).click();
        await expect(page.getByTestId('net-result')).toBeVisible();
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
        expect(clipped, 'ellipsized text on the Network page').toEqual([]);
        const pageScroll = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
        expect(pageScroll).toBe(false);
      });
    }
  });
}
