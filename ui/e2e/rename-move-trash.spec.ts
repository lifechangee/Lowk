// Library management (mock IPC), both languages: rename a call / folder / document (in place with a click or F2,
// from the list menus), refuse an empty name, move a document to another folder, trashed documents in the Trash.
import { expect, test, type Page } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

const ACME = '01JC8Z3K6Q9X2M4N7R5T0V0001';
const EMPTY = '01JC8Z3K6Q9X2M4N7R5T0V0005';
const CALL1 = '01JC8Z3K6Q9X2M4N7R5T0V0101';
const file = (name: string, size = 2000) => ({ name, mimeType: 'application/octet-stream', buffer: Buffer.alloc(size, 1) });

const T = {
  en: { rename: 'Rename', empty: 'The name can’t be empty.', moveTo: 'Move to folder', trash: 'Trash', restore: 'Restore', del: 'Move to trash', add: /Add document/, actions: 'Call actions', docActions: 'Document actions', save: 'Save' },
  ru: { rename: 'Переименовать', empty: 'Название не может быть пустым.', moveTo: 'Перенести в папку', trash: 'Корзина', restore: 'Восстановить', del: 'В корзину', add: /Добавить документ/, actions: 'Действия со звонком', docActions: 'Действия с документом', save: 'Сохранить' },
} as const;

async function addDoc(page: Page, name: string) {
  const ch = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: T.en.add }).or(page.getByRole('button', { name: T.ru.add })).first().click();
  (await ch).setFiles(file(name));
  const row = page.locator('[data-doc]', { hasText: name.replace(/\.\w+$/, '') });
  await expect(row).toHaveAttribute('data-status', 'ready', { timeout: 10_000 });
  return row;
}

for (const lang of ['en', 'ru'] as const) {
  const t = T[lang];
  test.describe(`rename / move / trash ${lang}`, () => {
    test('a call is renamed in place by click, F2 and the menu; empty is refused; Esc cancels', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}#/call/${CALL1}`);
      const title = page.getByTestId('call-title');
      const old = (await title.innerText()).trim();
      // click -> edit -> Enter
      await title.getByRole('button').click();
      const input = page.getByTestId('call-title-input');
      await expect(input).toBeFocused();
      await input.fill('Первая версия');
      await input.press('Escape');
      await expect(title).toHaveText(old);
      // F2 -> empty refused with a message, field stays
      await page.keyboard.press('F2');
      await input.fill('   ');
      await input.press('Enter');
      await expect(page.getByTestId('call-title-error')).toHaveText(t.empty);
      await expect(input).toBeVisible();
      await input.fill('  Итоговая встреча  ');
      await input.press('Enter');
      await expect(page.getByTestId('call-title')).toHaveText('Итоговая встреча');
      // the sidebar-less lists follow: the folder list shows the new title
      await page.goto(`/main.html?lang=${lang}#/folder/${ACME}`);
      await expect(page.locator('.tbl .tr', { hasText: 'Итоговая встреча' })).toHaveCount(1);
      // menu on the page
      await page.goto(`/main.html?lang=${lang}#/call/${CALL1}`);
      await page.getByTestId('call-menu').click();
      await page.getByRole('menuitem', { name: t.rename }).click();
      await page.getByTestId('call-title-input').fill('Из меню');
      await page.getByTestId('call-title-input').press('Enter');
      await expect(page.getByTestId('call-title')).toHaveText('Из меню');
    });

    test('«Переименовать» in the call list menu opens the dialog; duplicates are fine', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}#/folder/${ACME}`);
      const rows = page.locator('.tbl .tr');
      await rows.first().getByRole('button', { name: t.actions }).click();
      await page.getByRole('menuitem', { name: t.rename }).click();
      const input = page.getByTestId('rename-input');
      await input.fill('');
      await page.locator('.modal').getByRole('button', { name: t.save }).click();
      await expect(page.getByTestId('rename-error')).toHaveText(t.empty);
      await input.fill('Одинаковое');
      await input.press('Enter');
      await expect(rows.filter({ hasText: 'Одинаковое' })).toHaveCount(1);
      await rows.nth(1).getByRole('button', { name: t.actions }).click();
      await page.getByRole('menuitem', { name: t.rename }).click();
      await page.getByTestId('rename-input').fill('Одинаковое');
      await page.getByTestId('rename-input').press('Enter');
      await expect(rows.filter({ hasText: 'Одинаковое' })).toHaveCount(2);
    });

    test('a folder is renamed from its page and from the sidebar menu', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}#/folder/${ACME}`);
      await page.getByTestId('folder-title').getByRole('button').click();
      await page.getByTestId('folder-title-input').fill('Клиенты');
      await page.getByTestId('folder-title-input').press('Enter');
      await expect(page.getByTestId('folder-row').filter({ hasText: 'Клиенты' })).toBeVisible();
      await expect(page.getByTestId('folder-row').filter({ hasText: 'Acme Corp' })).toHaveCount(0);
      const row = page.getByTestId('folder-row').filter({ hasText: 'Клиенты' });
      await row.hover();
      await row.getByTestId('folder-row-menu').click();
      await page.getByRole('menuitem', { name: t.rename }).click();
      await page.getByTestId('rename-input').fill('Заказчики');
      await page.getByTestId('rename-input').press('Enter');
      await expect(page.getByTestId('folder-title')).toHaveText('Заказчики');
      await page.goto(`/main.html?lang=${lang}#/folder/sys:unsorted`);
      await expect(page.getByTestId('folder-title-input')).toHaveCount(0);
      await page.getByTestId('folder-title').click();
      await expect(page.getByTestId('folder-title-input')).toHaveCount(0); // system folders have fixed names
    });

    test('a document: rename from the row menu and on its page, move to another folder, delete and restore from the Trash', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}#/folder/${EMPTY}/documents`);
      const row = await addDoc(page, 'Handbook.pdf');
      // list menu -> dialog
      await row.getByRole('button', { name: t.docActions }).click();
      await page.getByRole('menuitem', { name: t.rename }).click();
      await page.getByTestId('rename-input').fill('Руководство');
      await page.getByTestId('rename-input').press('Enter');
      const renamed = page.locator('[data-doc]', { hasText: 'Руководство' });
      await expect(renamed).toBeVisible();
      // page title, inline
      await renamed.locator('a.ttl').click();
      await page.getByTestId('doc-title').getByRole('button').click();
      await page.getByTestId('doc-title-input').fill('Руководство 2');
      await page.getByTestId('doc-title-input').press('Enter');
      await expect(page.getByTestId('doc-title')).toHaveText('Руководство 2');
      // move from the page menu
      await page.getByTestId('doc-menu').click();
      await page.getByRole('menuitem', { name: t.moveTo }).click();
      await page.getByRole('menu', { name: /Move to…|Переместить в…/ }).getByRole('menuitem', { name: 'Acme Corp' }).click();
      await page.goto(`/main.html?lang=${lang}#/folder/${ACME}/documents`);
      const moved = page.locator('[data-doc]', { hasText: 'Руководство 2' });
      await expect(moved).toBeVisible();
      // and from the row menu back
      await moved.getByRole('button', { name: t.docActions }).click();
      await page.getByRole('menuitem', { name: t.moveTo }).click();
      await page.getByRole('menu', { name: /Move to…|Переместить в…/ }).getByRole('menuitem', { name: 'Новый проект' }).click();
      await expect(moved).toHaveCount(0);
      await page.goto(`/main.html?lang=${lang}#/folder/${EMPTY}/documents`);
      const back = page.locator('[data-doc]', { hasText: 'Руководство 2' });
      await expect(back).toBeVisible();
      // delete -> Trash lists the document -> restore
      const badge = page.locator('.side a', { hasText: t.trash }).locator('.n');
      const before = Number(await badge.innerText());
      await back.getByRole('button', { name: t.docActions }).click();
      await page.getByRole('menuitem', { name: t.del }).click();
      await expect(back).toHaveCount(0);
      await page.locator('.side a', { hasText: t.trash }).click();
      const td = page.getByTestId('trash-doc');
      await expect(td).toHaveCount(1);
      await expect(td).toContainText('Руководство 2');
      await expect(badge).toHaveText(String(before + 1));
      await td.getByRole('button', { name: t.restore }).click();
      await expect(badge).toHaveText(String(before));
      await expect(page.getByTestId('trash-doc')).toHaveCount(0);
      await page.goto(`/main.html?lang=${lang}#/folder/${EMPTY}/documents`);
      await expect(page.locator('[data-doc]', { hasText: 'Руководство 2' })).toBeVisible();
    });
  });
}
