// «Перераспознать запись» (mock IPC): the call-page action, progress, cancel, pause while recording, the live version
// kept and restorable, the settings and the folder preset. Both languages.
import { expect, test } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

const ACME = '01JC8Z3K6Q9X2M4N7R5T0V0001';
const CALL1 = '01JC8Z3K6Q9X2M4N7R5T0V0101'; // has saved audio in the mock
const CALL4 = '01JC8Z3K6Q9X2M4N7R5T0V0104'; // text only

test('a call with saved audio offers the action, shows progress, writes the new text and keeps the live one', async ({ page }) => {
  await page.goto(`/main.html?lang=en#/call/${CALL1}`);
  const bar = page.getByTestId('retx-bar');
  await expect(bar).toBeVisible();
  const first = page.locator('[data-line="0"] .tx');
  const live = await first.innerText();
  expect(live).not.toContain('(re-transcribed)');

  await bar.getByRole('button', { name: 'Re-transcribe recording' }).click();
  await expect(page.getByTestId('retx-progress')).toBeVisible();
  await expect(page.getByTestId('retx-state')).toContainText('Re-transcribing');
  await expect(bar.getByRole('button', { name: 'Cancel' })).toBeVisible();

  await expect(page.getByTestId('retx-rev')).toContainText('version 2', { timeout: 10_000 });
  await expect(first).toContainText('(re-transcribed)');
  await expect(page.getByTestId('retx-progress')).toHaveCount(0);

  // the live version is restorable, nothing was destroyed
  await page.getByTestId('retx-restore').click();
  await expect(first).toHaveText(live);
});

test('cancel leaves the transcript as it was and brings the button back', async ({ page }) => {
  await page.goto(`/main.html?lang=en&retx=hold#/call/${CALL1}`);
  const first = page.locator('[data-line="0"] .tx');
  const live = await first.innerText();
  await page.getByTestId('retx-bar').getByRole('button', { name: 'Re-transcribe recording' }).click();
  await expect(page.getByTestId('retx-progress')).toBeVisible();
  await page.getByTestId('retx-bar').getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByTestId('retx-progress')).toHaveCount(0);
  await expect(page.getByTestId('retx-bar').getByRole('button', { name: 'Re-transcribe recording' })).toBeVisible();
  await expect(first).toHaveText(live);
  await expect(page.getByTestId('retx-rev')).toHaveCount(0);
});

test('a recording that starts meanwhile pauses the pass, which resumes and finishes after it', async ({ page }) => {
  await page.goto(`/main.html?lang=en&retx=pause#/call/${CALL1}`);
  await page.getByTestId('retx-bar').getByRole('button', { name: 'Re-transcribe recording' }).click();
  await expect(page.getByTestId('retx-state')).toContainText('A recording is running');
  await expect(page.getByTestId('retx-rev')).toContainText('version 2', { timeout: 10_000 });
});

test('no saved audio, no action', async ({ page }) => {
  await page.goto(`/main.html?lang=en#/call/${CALL4}`);
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  await expect(page.getByTestId('retx-bar')).toHaveCount(0);
  await page.goto(`/main.html?lang=en&retx=none#/call/${CALL1}`);
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  await expect(page.getByTestId('retx-bar')).toHaveCount(0);
});

test('a local-only folder refusal and a missing model are explained', async ({ page }) => {
  await page.goto(`/main.html?lang=en&retx=localonly#/call/${CALL1}`);
  await page.getByTestId('retx-bar').getByRole('button', { name: 'Re-transcribe recording' }).click();
  await expect(page.getByText(/local-only/)).toBeVisible();
  await expect(page.getByTestId('retx-progress')).toHaveCount(0);
  await page.goto(`/main.html?lang=en&retx=nomodel#/call/${CALL1}`);
  await page.getByTestId('retx-bar').getByRole('button', { name: 'Re-transcribe recording' }).click();
  await expect(page.getByText(/Download the model/)).toBeVisible();
});

test('Russian strings', async ({ page }) => {
  await page.goto(`/main.html?lang=ru#/call/${CALL1}`);
  const bar = page.getByTestId('retx-bar');
  await expect(bar.getByRole('button', { name: 'Перераспознать запись' })).toBeVisible();
  await expect(bar).toContainText('Живая версия сохраняется');
  await bar.getByRole('button', { name: 'Перераспознать запись' }).click();
  await expect(page.getByTestId('retx-state')).toContainText('Перераспознаём');
  await expect(page.getByTestId('retx-rev')).toContainText('версия 2', { timeout: 10_000 });
  await expect(page.getByTestId('retx-restore')).toHaveText('Вернуть живую версию');
});

test('settings: after-recording and Deepgram switches (both off by default); folder preset inherit/on/off', async ({ page }) => {
  await page.goto('/main.html?lang=en#/settings/general');
  const after = page.getByRole('switch', { name: 'Re-transcribe after recording' });
  const cloud = page.getByRole('switch', { name: 'Re-transcribe with Deepgram' });
  await expect(after).toHaveAttribute('aria-checked', 'false');
  await expect(cloud).toHaveAttribute('aria-checked', 'false');
  await after.click();
  await expect(after).toHaveAttribute('aria-checked', 'true');
  await expect(page.getByText('Local-only folders are never sent to the cloud.')).toBeVisible();

  await page.goto(`/main.html?lang=en#/folder/${ACME}`);
  const sel = page.getByTestId('folder-retx').locator('select');
  await expect(sel).toHaveValue('inherit');
  await sel.selectOption('on');
  await expect(sel).toHaveValue('on');
});
