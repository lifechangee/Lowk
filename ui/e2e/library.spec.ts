// Library management (mock IPC): delete a folder (Trash, restore, recording guard), «В корзину» from call lists
// with undo, and no tag UI anywhere. Both UI languages.
import { expect, test, type Page } from '@playwright/test';
import { lintInPage, type Issue } from './lib/lint';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

const ACME = '01JC8Z3K6Q9X2M4N7R5T0V0001';
const EMPTY = '01JC8Z3K6Q9X2M4N7R5T0V0005';
const CALL1 = '01JC8Z3K6Q9X2M4N7R5T0V0101';

const T = {
  en: {
    del: 'Delete folder', confirm: /Folder “Acme Corp”: 4 calls(, \d+ documents?)? will be moved to the trash/, trash: 'Trash',
    toTrash: 'Move to trash', moved: 'Moved to trash', undo: 'Undo', restore: 'Restore', actions: 'Call actions',
    deleted: 'Folder “Acme Corp” moved to trash', restored: 'Folder “Acme Corp” restored',
  },
  ru: {
    del: 'Удалить папку', confirm: /Папка «Acme Corp»: 4 созвона(, \d+ документ\S*)? будут перемещены в корзину/, trash: 'Корзина',
    toTrash: 'В корзину', moved: 'Перемещено в корзину', undo: 'Отменить', restore: 'Восстановить', actions: 'Действия со звонком',
    deleted: 'Папка «Acme Corp» перемещена в корзину', restored: 'Папка «Acme Corp» восстановлена',
  },
} as const;

const folderRow = (page: Page, title: string) => page.getByTestId('folder-row').filter({ hasText: title });

for (const lang of ['en', 'ru'] as const) {
  const t = T[lang];

  test.describe(`library ${lang}`, () => {
    test('folder delete -> Trash -> restore brings the folder and its calls back', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}#/folder/${ACME}`);
      await expect(folderRow(page, 'Acme Corp')).toBeVisible();
      await page.getByTestId('folder-menu').click();
      await page.getByRole('menuitem', { name: t.del }).click();
      const dlg = page.getByTestId('folder-delete-confirm');
      await expect(dlg).toBeVisible();
      await expect(page.getByTestId('folder-delete-body')).toHaveText(t.confirm);
      const issues = (await page.evaluate(lintInPage, { root: '.modal' })) as Issue[];
      expect(issues.filter((i) => ['overflow', 'outside', 'text-overlap', 'flex-gap'].includes(i.rule))).toEqual([]);
      await page.locator('.modal').getByRole('button', { name: t.del }).click();

      await expect(folderRow(page, 'Acme Corp')).toHaveCount(0);
      await expect(page.locator('.toast, [role=status]').filter({ hasText: t.deleted }).first()).toBeVisible();
      await page.locator('.side a', { hasText: t.trash }).click();
      const tf = page.getByTestId('trash-folder');
      await expect(tf).toHaveCount(1);
      await expect(tf).toContainText('Acme Corp');
      // the folder's calls are listed under the folder, not as separate rows
      await expect(page.getByText('Квартальное планирование Q4')).toHaveCount(0);
      await tf.getByRole('button', { name: t.restore }).click();
      await expect(folderRow(page, 'Acme Corp')).toBeVisible();
      await expect(page.getByTestId('trash-folder')).toHaveCount(0);
      await page.goto(`/main.html?lang=${lang}#/folder/${ACME}`);
      await expect(page.locator('.tbl .tr')).toHaveCount(4);
    });

    test('the sidebar folder menu deletes too, and Undo in the toast restores', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}#/`);
      const row = folderRow(page, 'Acme Corp');
      await row.hover();
      await row.getByTestId('folder-row-menu').click();
      await page.getByRole('menuitem', { name: t.del }).click();
      await page.locator('.modal').getByRole('button', { name: t.del }).click();
      await expect(folderRow(page, 'Acme Corp')).toHaveCount(0);
      await page.getByRole('button', { name: t.undo }).click();
      await expect(folderRow(page, 'Acme Corp')).toBeVisible();
    });

    test('system folders (Unsorted, Trash) cannot be deleted', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}#/folder/sys:unsorted`);
      await expect(page.getByTestId('folder-menu')).toHaveCount(0);
      await page.goto(`/main.html?lang=${lang}#/folder/sys:trash`);
      await expect(page.getByTestId('folder-menu')).toHaveCount(0);
      await expect(page.getByTestId('folder-row')).toHaveCount(5); // user folders only
    });

    test('a folder with a recording in progress refuses to be deleted, with a clear message', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}#/folder/${EMPTY}`);
      await page.locator('[data-np-trigger] button').last().click();
      await page.locator('.np button.btn.primary').click();
      await page.waitForSelector('.recpill');
      const row = folderRow(page, 'Новый проект');
      await row.hover();
      await row.getByTestId('folder-row-menu').click();
      await page.getByRole('menuitem', { name: t.del }).click();
      await expect(page.getByTestId('folder-delete-recording')).toBeVisible();
      await expect(page.locator('.modal').getByRole('button', { name: t.del })).toBeDisabled();
      await expect(folderRow(page, 'Новый проект')).toBeVisible();
    });

    test('«В корзину» in the folder list and on Home, with Undo', async ({ page }) => {
      // folder call list
      await page.goto(`/main.html?lang=${lang}#/folder/${ACME}`);
      const rows = page.locator('.tbl .tr');
      await expect(rows).toHaveCount(4);
      await rows.first().getByRole('button', { name: t.actions }).click();
      await page.getByRole('menuitem', { name: t.toTrash }).click();
      await expect(rows).toHaveCount(3);
      await expect(page.getByText(t.moved).first()).toBeVisible();
      await page.getByRole('button', { name: t.undo }).click();
      await expect(rows).toHaveCount(4);
      // home recent list; «Переместить» is still there
      await page.goto(`/main.html?lang=${lang}#/`);
      const recent = page.locator('.tbl .tr');
      const title = (await recent.first().locator('b').first().innerText()).trim();
      await recent.first().getByRole('button', { name: t.actions }).click();
      await expect(page.getByRole('menu').getByRole('menuitem')).not.toHaveCount(1);
      await page.getByRole('menuitem', { name: t.toTrash }).click();
      await expect(recent.filter({ hasText: title })).toHaveCount(0);
      await page.getByRole('button', { name: t.undo }).click();
      await expect(recent.filter({ hasText: title })).toHaveCount(1);
    });
  });
}

test('no tag UI in the main window pages (control test ids, texts, filters)', async ({ page }) => {
  const TAG_IDS = ['tag-editor', 'tag-suggestions', 'tag-add', 'tag-filter', 'tag-chip'];
  const hashes = ['#/', `#/folder/${ACME}`, `#/folder/${ACME}/documents`, `#/folder/${ACME}/instructions`, '#/folder/sys:unsorted', '#/folder/sys:trash',
    `#/call/${CALL1}`, '#/search?q=EBITDA', '#/settings/general', '#/settings/providers', '#/settings/knowledge'];
  for (const lang of ['en', 'ru']) {
    for (const h of hashes) {
      await page.goto(`/main.html?lang=${lang}${h.includes('?') ? '&' : ''}${h.replace(/^#/, '#')}`);
      await page.waitForSelector('.side');
      if (h.startsWith('#/call/')) await page.getByRole('tab').nth(1).click();
      await page.waitForTimeout(150);
      const html = await page.content();
      for (const id of TAG_IDS) expect(html, `${h} ${lang} has data-testid=${id}`).not.toContain(`data-testid="${id}"`);
      const text = await page.locator('body').innerText();
      expect(text, `${h} ${lang} shows tags`).not.toMatch(/\+ тег|\+ tag|Новый тег|New tag|#продажи|#финансы|# продажи|Предложить теги|Suggest tags/);
      expect(text).not.toMatch(/\b(Tags|Теги)\b/);
    }
  }
});
