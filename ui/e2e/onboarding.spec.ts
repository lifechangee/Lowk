// Onboarding end to end on the mac and Windows variants, RU + EN (mock IPC): every step, the legal-notice gate,
// skip / back, resume after a restart, the permission rows, the model download, the keys and the folder.
import { expect, test, type Page } from '@playwright/test';
import { lintInPage, type Issue } from './lib/lint';

test.use({ viewport: { width: 1280, height: 800 }, timezoneId: 'UTC' });

type Os = 'macos' | 'windows';
type Lang = 'en' | 'ru';
const T = {
  en: { legalTitle: /Recording conversations and the law/, ack: /I understand and take responsibility/, next: 'Continue', back: 'Back', skip: 'Skip', finish: 'Get started', open: 'Open settings', allowAll: 'Allow all', install: /Download/, installed: 'Installed', change: 'Change', done: 'Done', skipped: 'skipped' },
  ru: { legalTitle: /Запись разговоров и закон/, ack: /Я понимаю и беру ответственность/, next: 'Продолжить', back: 'Назад', skip: 'Пропустить', finish: 'Начать работу', open: 'Открыть настройки', allowAll: 'Разрешить всё', install: /Скачать/, installed: 'Установлена', change: 'Изменить', done: 'Готово', skipped: 'пропущено' },
} as const;

const lint = async (page: Page, where: string) => {
  await page.waitForTimeout(450); // the step's 0.3s slide-in
  const issues = ((await page.evaluate(lintInPage, { root: '.win' })) as Issue[]).filter((i) => i.rule !== 'grid');
  expect(issues, `layout lint at ${where}`).toEqual([]);
};
const stepNo = (page: Page) => page.evaluate(() => Number(location.hash.split('/').pop()));
const nextBtn = (page: Page, l: Lang) => page.locator('.nav').getByRole('button', { name: T[l].next, exact: true });

for (const os of ['macos', 'windows'] as Os[]) {
  for (const lang of ['en', 'ru'] as Lang[]) {
    const t = T[lang];
    test(`onboarding ${os}/${lang}: all nine steps, legal gate, permissions, model download, keys, folder, finish`, async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}&os=${os}&fresh`);
      await page.waitForSelector('.win');
      expect(await stepNo(page)).toBe(0);
      await lint(page, 'language');
      await nextBtn(page, lang).click();

      // 1: recording-law notice must be acknowledged; Next stays disabled until then
      await expect(page.getByRole('heading', { level: 1 })).toHaveText(t.legalTitle);
      await expect(nextBtn(page, lang)).toBeDisabled();
      await expect(page.locator('.nav').getByRole('button', { name: t.skip, exact: true })).toHaveCount(0);
      await lint(page, 'legal');
      await page.getByRole('checkbox', { name: t.ack }).click();
      await expect(nextBtn(page, lang)).toBeEnabled();
      await nextBtn(page, lang).click();

      // 2: audio source
      expect(await stepNo(page)).toBe(2);
      await lint(page, 'capture');
      await nextBtn(page, lang).click();

      // 3: permissions: mac shows all five with «Open settings»; Windows only the microphone, with a note
      expect(await stepNo(page)).toBe(3);
      const rows = page.locator('[data-testid^=perm-]');
      if (os === 'macos') {
        await expect(rows).toHaveCount(5);
        await expect(page.getByRole('button', { name: t.open, exact: true })).toHaveCount(5);
        await page.getByTestId('perm-screen_recording').getByRole('button', { name: t.open, exact: true }).click();
        expect(await page.evaluate(() => (globalThis as { __permOpened?: string[] }).__permOpened)).toContain('screen_recording');
        await lint(page, 'permissions');
        await page.getByRole('button', { name: t.allowAll }).click();
        await expect(page.locator('[data-testid^=perm-] .pill')).toHaveCount(5, { timeout: 5000 });
      } else {
        await expect(rows).toHaveCount(1);
        await expect(page.getByTestId('perm-microphone')).toBeVisible();
        await expect(page.locator('.note').filter({ hasText: lang === 'en' ? 'only the microphone' : 'только микрофон' })).toBeVisible();
        await lint(page, 'permissions');
      }
      await nextBtn(page, lang).click();

      // 4: hardware + engine; the model downloads right here
      expect(await stepNo(page)).toBe(4);
      await expect(page.getByText(os === 'macos' ? 'Apple M4' : 'Intel Core i5-10400F')).toBeVisible();
      const card = page.getByTestId('engine-card');
      await expect(card).toBeVisible();
      await card.getByRole('button', { name: t.install }).click();
      await expect(card.getByRole('progressbar')).toBeVisible();
      await expect(card).toContainText(t.installed, { timeout: 5000 });
      await lint(page, 'hardware');
      await nextBtn(page, lang).click();

      // 5: optional keys: both cards, skippable
      expect(await stepNo(page)).toBe(5);
      await expect(page.locator('input')).toHaveCount(2);
      await page.locator('input').first().fill('sk-or-v1-test');
      await page.locator('.card, [data-testid]').getByRole('button').first().waitFor();
      await lint(page, 'keys');
      await nextBtn(page, lang).click();

      // 6: folder
      expect(await stepNo(page)).toBe(6);
      await expect(page.getByTestId('onb-vault')).toContainText('Documents/Lowk');
      await page.getByTestId('onb-vault').getByRole('button', { name: t.change, exact: true }).click();
      await expect(page.getByTestId('onb-vault')).toContainText(os === 'windows' ? 'D:\\Notes\\Lowk' : 'Obsidian/Lowk');
      await lint(page, 'folder');
      await nextBtn(page, lang).click();

      // 7: hotkeys summary
      expect(await stepNo(page)).toBe(7);
      await lint(page, 'hotkeys');
      await page.locator('.nav').getByRole('button', { name: t.done, exact: true }).click();

      // 8: finish
      expect(await stepNo(page)).toBe(8);
      await lint(page, 'done');
      await page.getByRole('button', { name: t.finish }).click();
      await expect(page).toHaveURL(/#\/$/);
      await expect(page.locator('.win')).toHaveCount(0);
    });
  }
}

test('onboarding: skip and back never dead-end; skipped steps are marked on the last page', async ({ page }) => {
  await page.goto('/main.html?lang=en&fresh&ack&step=2');
  await page.waitForSelector('.win');
  expect(await stepNo(page)).toBe(2);
  for (const n of [3, 4, 5, 6, 7]) {
    await page.locator('.nav').getByRole('button', { name: 'Skip', exact: true }).click();
    expect(await stepNo(page)).toBe(n);
  }
  await page.locator('.nav').getByRole('button', { name: 'Skip', exact: true }).click();
  expect(await stepNo(page)).toBe(8);
  await expect(page.locator('.sum')).toContainText('skipped');
  for (const n of [7, 6, 5]) {
    await page.locator('.nav').getByRole('button', { name: 'Back', exact: true }).click();
    expect(await stepNo(page)).toBe(n);
  }
  await page.locator('.steps .st').nth(0).click(); // the rail jumps back to any finished step
  expect(await stepNo(page)).toBe(0);
});

test('onboarding resumes at the stored step after a restart, and the legal notice cannot be skipped by a stale step', async ({ page }) => {
  await page.goto('/main.html?lang=en&fresh&ack&step=4#/');
  await page.waitForSelector('.win');
  expect(await stepNo(page)).toBe(4);
  await page.goto('/main.html?lang=ru&fresh&step=6#/');
  await page.waitForSelector('.win');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(/Запись разговоров и закон/);
  expect(await stepNo(page)).toBe(1);
});
