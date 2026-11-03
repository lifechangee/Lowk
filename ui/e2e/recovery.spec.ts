// G31 recovery notices (mock IPC): crash-recovery notice that opens the call, a missing call, unsaved document edits.
import { expect, test } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

const CALL1 = '01JC8Z3K6Q9X2M4N7R5T0V0101';
const DOC = '01JC8Z3K6Q9X2M4N7R5T0V0401';

test('a call recovered after a crash is announced once and the notice opens it', async ({ page }) => {
  await page.goto('/main.html?lang=en&recovered=1#/');
  const toast = page.locator('.toast');
  await expect(toast).toContainText('was interrupted by a crash and has been recovered');
  await expect(toast).toContainText('Saved: 13 min');
  await toast.getByRole('button', { name: 'Open' }).click();
  await expect(page).toHaveURL(/#\/call\/.+/);
});

test('opening a call that no longer exists shows «not found» with a way back', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await page.goto('/main.html?lang=en#/call/01JC8Z3K6Q9X2M4N7R5T0V9999');
  const s = page.getByTestId('call-missing');
  await expect(s).toContainText('Call not found');
  await s.getByRole('button', { name: 'Back' }).click();
  await expect(page.getByTestId('call-missing')).toHaveCount(0);
});

test.describe('unsaved document edits', () => {
  async function edit(page: import('@playwright/test').Page) {
    await page.goto(`/main.html?lang=en#/doc/${DOC}`);
    await page.getByRole('button', { name: 'Edit' }).click();
    await page.locator('.cm-content, textarea.fb').first().click();
    await page.keyboard.type('hello ');
    await expect(page.getByText('Unsaved').first()).toBeVisible();
  }

  test('leaving asks; Stay keeps the page and the edit', async ({ page }) => {
    await edit(page);
    await page.evaluate((id) => { location.hash = `#/call/${id}`; }, CALL1);
    const dlg = page.getByRole('dialog', { name: 'Save the document changes?' });
    await expect(dlg).toBeVisible();
    await dlg.getByRole('button', { name: 'Stay' }).click();
    await expect(page).toHaveURL(new RegExp(`#/doc/${DOC}`));
    await expect(page.getByText('Unsaved').first()).toBeVisible();
  });

  test('Do not save leaves', async ({ page }) => {
    await edit(page);
    await page.evaluate((id) => { location.hash = `#/call/${id}`; }, CALL1);
    await page.getByRole('dialog').getByRole('button', { name: 'Do not save' }).click();
    await expect(page).toHaveURL(new RegExp(`#/call/${CALL1}`));
  });

  test('Save saves, then leaves', async ({ page }) => {
    await edit(page);
    await page.evaluate((id) => { location.hash = `#/call/${id}`; }, CALL1);
    await page.getByRole('dialog').getByRole('button', { name: 'Save', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`#/call/${CALL1}`));
  });

  test('no edit, no question', async ({ page }) => {
    await page.goto(`/main.html?lang=en#/doc/${DOC}`);
    await page.evaluate((id) => { location.hash = `#/call/${id}`; }, CALL1);
    await expect(page).toHaveURL(new RegExp(`#/call/${CALL1}`));
  });
});

test('unsaved edits are guarded again on a second visit to a document', async ({ page }) => {
  for (let i = 0; i < 2; i++) {
    await page.goto(`/main.html?lang=en#/doc/${DOC}`);
    await page.getByRole('button', { name: 'Edit' }).click();
    await page.locator('.cm-content, textarea.fb').first().click();
    await page.keyboard.type('hello ');
    await page.evaluate((id) => { location.hash = `#/call/${id}`; }, CALL1);
    await expect(page.getByRole('dialog', { name: 'Save the document changes?' })).toBeVisible();
    await page.getByRole('dialog').getByRole('button', { name: 'Do not save' }).click();
    await expect(page).toHaveURL(new RegExp(`#/call/${CALL1}`));
  }
});

test('Stay puts the address back, and the page keeps working', async ({ page }) => {
  await page.goto(`/main.html?lang=en#/doc/${DOC}`);
  await page.getByRole('button', { name: 'Edit' }).click();
  await page.locator('.cm-content, textarea.fb').first().click();
  await page.keyboard.type('hello ');
  await page.evaluate((id) => { location.hash = `#/call/${id}`; }, CALL1);
  await page.getByRole('dialog').getByRole('button', { name: 'Stay' }).click();
  await expect(page).toHaveURL(new RegExp(`#/doc/${DOC}$`));
  await page.evaluate((id) => { location.hash = `#/call/${id}`; }, CALL1);
  await expect(page.getByRole('dialog')).toBeVisible();
});
