// Onboarding end to end on the mac and Windows variants, RU + EN (mock IPC): every step, the legal-notice gate,
// skip / back, resume after a restart, the permission rows, the model download, the keys and the folder.
import { expect, test, type Page } from '@playwright/test';
import { lintInPage, type Issue } from './lib/lint';

test.use({ viewport: { width: 1280, height: 800 }, timezoneId: 'UTC' });

type Os = 'macos' | 'windows';
type Lang = 'en' | 'ru';
const T = {
  en: { legalTitle: /Recording conversations and the law/, ack: /I understand and take responsibility/, next: 'Continue', back: 'Back', skip: 'Skip', finish: 'Get started', open: 'Open settings', allowAll: 'Allow all', install: /Download/, installed: 'Ready', change: 'Change', done: 'Done', skipped: 'skipped' },
  ru: { legalTitle: /Запись разговоров и закон/, ack: /Я понимаю и беру ответственность/, next: 'Продолжить', back: 'Назад', skip: 'Пропустить', finish: 'Начать работу', open: 'Открыть настройки', allowAll: 'Разрешить всё', install: /Скачать/, installed: 'Готова', change: 'Изменить', done: 'Готово', skipped: 'пропущено' },
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
      await expect(page.getByText(os === 'macos' ? 'Apple M4' : 'i5-12400F')).toBeVisible();
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

for (const os of ['macos', 'windows'] as Os[]) {
  test(`onboarding step 3 (${os}): both capture options, whole system preselected, the app picker is back, the choice is kept`, async ({ page }) => {
    await page.goto(`/main.html?lang=en&os=${os}&fresh&ack#/onboarding/2`);
    await page.waitForSelector('.win');
    const system = page.locator('.opt', { hasText: 'Whole system audio' });
    const app = page.locator('.opt', { hasText: 'One app' });
    await expect(system).toHaveAttribute('aria-checked', 'true');
    await expect(app).toHaveAttribute('aria-checked', 'false');
    await expect(page.locator('.apps [data-source-value]')).toHaveCount(1);
    const note = page.getByTestId('capture-win-note');
    if (os === 'windows') await expect(note).toBeVisible(); else await expect(note).toHaveCount(0);
    await lint(page, `capture ${os}`);
    await app.click();
    await expect(app).toHaveAttribute('aria-checked', 'true');
    await expect(system).toHaveAttribute('aria-checked', 'false');
    expect(await page.locator('.apps [data-source-value]').getAttribute('data-source-value')).toMatch(/^app:.+/);
    await system.click();
    await expect(system).toHaveAttribute('aria-checked', 'true');
    await app.click();
    await page.locator('.nav').getByRole('button', { name: 'Continue', exact: true }).click();
    expect(await stepNo(page)).toBe(3);
    await page.evaluate(() => (location.hash = '#/onboarding/8'));
    await expect(page.locator('.sum')).not.toContainText('Whole system');
    // the New call popover still starts with the whole system
    await page.getByRole('button', { name: 'Get started' }).click();
    await page.locator('.side .nc button.btn.primary').click();
    await expect(page.locator('.np [data-source-value]')).toHaveAttribute('data-source-value', 'system');
  });

  for (const width of [1000, 1280]) {
    for (const lang of ['en', 'ru'] as Lang[]) {
      test(`onboarding step 8 hotkeys (${os}/${lang}/${width}px): layout lint, chips aligned with labels, no wrapping`, async ({ page }) => {
        await page.setViewportSize({ width, height: 800 });
        await page.goto(`/main.html?lang=${lang}&os=${os}&fresh&ack#/onboarding/7`);
        await page.waitForSelector('.win');
        await lint(page, `hotkeys ${os}/${lang}/${width}`);
        const rows = page.locator('.be .hk');
        expect(await rows.count()).toBe(8);
        const geo = await rows.evaluateAll((els) => els.map((el) => {
          const r = (e: Element) => e.getBoundingClientRect();
          const l = el.querySelector('.l')!, k = el.querySelector('.key')!;
          const kbds = [...k.querySelectorAll('kbd')].map((x) => r(x));
          const key = r(k);
          return {
            dy: Math.abs(r(l).top + r(l).height / 2 - (key.top + key.height / 2)),
            keyLeft: Math.round(key.left), keyW: Math.round(key.width),
            oneLine: kbds.every((b) => Math.abs(b.top - kbds[0]!.top) < 2 && b.height <= 18),
            inside: kbds.every((b) => b.left >= key.left - 0.5 && b.right <= key.right + 0.5),
            labelFits: (l as HTMLElement).scrollWidth <= (l as HTMLElement).clientWidth + 1,
          };
        }));
        for (const g of geo) {
          expect(g.dy, 'chip vertical centre vs its label').toBeLessThanOrEqual(2);
          expect(g.oneLine, 'chip parts stay on one line').toBe(true);
          expect(g.inside, 'chip parts stay inside the chip').toBe(true);
          expect(g.labelFits, 'label is not truncated').toBe(true);
        }
        expect(new Set(geo.map((g) => g.keyLeft)).size, 'all chips share one left edge').toBe(1);
        expect(new Set(geo.map((g) => g.keyW)).size, 'all chips share one width').toBe(1);
        await expect(page.locator('.be .hk[data-slot=ask_brief] .key')).toContainText(os === 'macos' ? '⌘' : 'Ctrl');
        if (os === 'windows') await expect(page.locator('.be .key').filter({ hasText: '⌘' })).toHaveCount(0);
      });
    }
  }
}
