// Settings → Templates flows on the mock IPC, RU and EN: create, validate, edit, duplicate, reorder, default, delete, reset.
import { expect, test, type Page } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, timezoneId: 'UTC' });

const L = {
  en: { newBtn: 'New template', save: 'Save', dup: 'Duplicate', del: 'Delete', reset: 'Reset to defaults', resetDo: 'Reset', mkdef: 'Make default', def: 'default', ru: 'Name (Russian)', nm: 'Name (English)', ins: 'Task text', taken: 'A template with this name already exists.', empty: 'Enter a name.' },
  ru: { newBtn: 'Новый шаблон', save: 'Сохранить', dup: 'Дублировать', del: 'Удалить', reset: 'Сбросить к стандартным', resetDo: 'Сбросить', mkdef: 'Сделать по умолчанию', def: 'по умолчанию', ru: 'Название (русский)', nm: 'Название (English)', ins: 'Текст задания', taken: 'Шаблон с таким названием уже есть.', empty: 'Введите название.' },
} as const;

async function open(page: Page, lang: 'ru' | 'en') {
  await page.goto(`/main.html?lang=${lang}#/settings/templates`);
  await page.waitForSelector('[data-testid=tpl-list]');
}
const order = (page: Page) => page.locator('[data-testid=tpl-list] > li').evaluateAll((l) => l.map((e) => (e as HTMLElement).dataset.tpl));
const name = (page: Page, id: string) => page.locator(`[data-tpl=${id}] .head .n`).innerText();

for (const lang of ['en', 'ru'] as const) {
  const t = L[lang];
  test.describe(`templates ${lang}`, () => {
    test('list: summary hidden, default marker, my-answer note', async ({ page }) => {
      await open(page, lang);
      expect(await order(page)).toEqual(['brief', 'complement', 'objection']);
      await expect(page.locator('[data-tpl=brief] .nm')).toContainText(t.def);
      await page.locator('[data-tpl=complement] .main').click();
      await expect(page.getByTestId('tpl-myanswer')).toBeVisible();
      await expect(page.locator('[data-tpl=complement]').getByRole('button', { name: t.del })).toHaveCount(0);
    });

    test('create with validation, then edit and persist', async ({ page }) => {
      await open(page, lang);
      await page.getByRole('button', { name: t.newBtn }).click();
      await page.getByTestId('tpl-editor').getByRole('button', { name: t.save }).click();
      await expect(page.getByRole('alert')).toContainText(t.empty);
      await page.getByLabel(t.nm, { exact: true }).fill('brief'); // taken (case-insensitive) in EN
      await page.getByLabel(t.ru, { exact: true }).fill(lang === 'ru' ? 'Кратко' : 'Моё');
      await page.getByLabel(t.ins, { exact: true }).fill('Do the thing');
      await expect(page.getByRole('alert')).toContainText(t.taken);
      await page.getByLabel(t.nm, { exact: true }).fill('Mine');
      await page.getByLabel(t.ru, { exact: true }).fill('Моё');
      await page.getByTestId('tpl-editor').getByRole('button', { name: t.save }).click();
      expect(await order(page)).toHaveLength(4);
      await expect(page.locator('[data-testid=tpl-list] > li').last()).toContainText(lang === 'ru' ? 'Моё' : 'Mine');
      await expect(page.locator('[data-testid=tpl-list] > li').last().locator('[data-testid=tpl-bind]')).toBeVisible();
    });

    test('duplicate, reorder, default, delete with confirm', async ({ page }) => {
      await open(page, lang);
      await page.locator('[data-tpl=objection] .main').click();
      await page.getByTestId('tpl-editor').getByRole('button', { name: t.dup }).click();
      const o = await order(page);
      expect(o).toHaveLength(4);
      expect(o[2]).toBe('objection');
      const copy = o[3]!;
      expect(await name(page, copy)).toContain(lang === 'ru' ? '(копия)' : '(copy)');
      // up/down
      await page.locator(`[data-tpl=${copy}] .mv button`).first().click();
      expect(await order(page)).toEqual(['brief', 'complement', copy, 'objection']);
      // default moves the marker
      await page.getByTestId('tpl-editor').getByRole('button', { name: t.mkdef }).click();
      await expect(page.locator(`[data-tpl=${copy}] .nm`)).toContainText(t.def);
      await expect(page.locator('[data-tpl=brief] .nm')).not.toContainText(t.def);
      // delete asks first; default falls back to brief
      await page.getByTestId('tpl-editor').getByRole('button', { name: t.del }).click();
      await page.locator('.modal').getByRole('button', { name: t.del }).click();
      expect(await order(page)).toEqual(['brief', 'complement', 'objection']);
      await expect(page.locator('[data-tpl=brief] .nm')).toContainText(t.def);
    });

    test('built-ins edit and reset', async ({ page }) => {
      await open(page, lang);
      await page.locator('[data-tpl=brief] .main').click();
      await expect(page.getByTestId('tpl-editor').getByRole('button', { name: t.del })).toHaveCount(0);
      await page.getByLabel(t.ins, { exact: true }).fill('Changed');
      await page.getByTestId('tpl-editor').getByRole('button', { name: t.save }).click();
      await expect(page.locator('[data-tpl=brief] .pv')).toHaveText('Changed');
      await page.getByRole('button', { name: t.reset }).click();
      await page.locator('.modal').getByRole('button', { name: t.resetDo }).click();
      await expect(page.locator('[data-tpl=brief] .pv')).not.toHaveText('Changed');
    });

    test('nothing is truncated or overflowing in the editor', async ({ page }) => {
      await page.setViewportSize({ width: 800, height: 700 });
      await open(page, lang);
      await page.locator('[data-tpl=objection] .main').click();
      const bad = await page.evaluate(() => [...document.querySelectorAll('[data-testid=tpl-editor] *, .tp .n, .tp .nm')].filter((e) => { const h = e as HTMLElement; return h.scrollWidth > h.clientWidth + 1 && getComputedStyle(h).overflowX !== 'visible' && h.tagName !== 'TEXTAREA' && h.tagName !== 'SELECT' && h.tagName !== 'INPUT'; }).map((e) => e.className));
      expect(bad).toEqual([]);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    });
  });
}
