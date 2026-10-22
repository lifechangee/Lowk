// M2 behaviour (mock IPC): documents, search, summary + tags, tour, embedder, export/import. Both languages for the tour.
import { expect, test, type Page } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

const ACME = '01JC8Z3K6Q9X2M4N7R5T0V0001';
const EMPTY = '01JC8Z3K6Q9X2M4N7R5T0V0005';
const CALL1 = '01JC8Z3K6Q9X2M4N7R5T0V0101';
const CALL4 = '01JC8Z3K6Q9X2M4N7R5T0V0104';
const file = (name: string, size = 2000) => ({ name, mimeType: 'application/octet-stream', buffer: Buffer.alloc(size, 1) });

/** The mock picks Meta binds on a Mac user agent (webkit) and Ctrl elsewhere (chromium reports Windows). */
const mod = (page: Page) => page.evaluate(() => (/Macintosh|Mac OS/.test(navigator.userAgent) ? 'Meta' : 'Control'));

async function addFile(page: Page, f: ReturnType<typeof file>) {
  const ch = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: /Add document/ }).first().click();
  (await ch).setFiles(f);
}

test.describe('documents', () => {
  test('a dropped file goes queued -> converting -> ready, shows progress, and can be pinned', async ({ page }) => {
    await page.goto(`/main.html?lang=en#/folder/${EMPTY}/documents`);
    await expect(page.getByTestId('docs-empty')).toBeVisible();
    await addFile(page, file('Handbook.pdf'));
    const row = page.locator('[data-doc]', { hasText: 'Handbook' });
    await expect(row).toBeVisible();
    await expect(row.locator('[role=progressbar]')).toBeVisible();
    await expect(row).toHaveAttribute('data-status', 'ready', { timeout: 10_000 });
    await row.getByRole('button', { name: 'Pin' }).click();
    await expect(row.getByRole('button', { name: 'Unpin' })).toBeVisible();
    await expect(page.getByTestId('pinned-meter')).toContainText('tokens');
  });

  test('an unsupported or encrypted file fails with a readable error, never an empty document', async ({ page }) => {
    await page.goto(`/main.html?lang=en#/folder/${EMPTY}/documents`);
    await addFile(page, file('archive.zip'));
    const bad = page.locator('[data-doc]', { hasText: 'archive' });
    await expect(bad).toHaveAttribute('data-status', 'failed');
    await expect(bad.getByRole('alert')).toContainText('not supported');
    await addFile(page, file('encrypted report.pdf'));
    const enc = page.locator('[data-doc]', { hasText: 'encrypted report' });
    await expect(enc).toHaveAttribute('data-status', 'failed', { timeout: 10_000 });
    await expect(enc.getByRole('alert')).toContainText('password');
    await expect(enc.getByRole('button', { name: 'Retry' })).toBeVisible();
  });

  test('a cancelled conversion can be retried; the pinned budget meter reports what did not fit', async ({ page }) => {
    await page.goto(`/main.html?lang=en#/folder/${ACME}/documents`);
    const busy = page.locator('[data-doc][data-status=converting]');
    await busy.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.locator('[data-doc]', { hasText: 'Отчёт по рынку' })).toHaveAttribute('data-status', 'cancelled');
    await page.goto('/main.html?lang=en#/folder/01JC8Z3K6Q9X2M4N7R5T0V0003/documents');
    await expect(page.getByTestId('pinned-meter')).toContainText('1 did not fit');
    await expect(page.locator('[data-doc]', { hasText: 'Scorecard' })).toContainText('Not sent');
  });

  test('the same file twice asks: open / upload as a copy / replace', async ({ page }) => {
    await page.goto(`/main.html?lang=en#/folder/${EMPTY}/documents`);
    await addFile(page, file('Contract.pdf'));
    await expect(page.locator('[data-doc]')).toHaveCount(1);
    await addFile(page, file('Contract.pdf'));
    const dlg = page.getByRole('dialog');
    await expect(dlg).toContainText('Already uploaded');
    await expect(dlg).toContainText('Contract.pdf');
    await expect(page.locator('[data-doc]')).toHaveCount(1);
    // copy
    await dlg.getByRole('button', { name: 'Upload as a copy' }).click();
    await expect(page.locator('[data-doc]')).toHaveCount(2);
    // replace: the old pair goes, one stays
    await addFile(page, file('Contract.pdf'));
    await page.getByRole('dialog').getByRole('button', { name: 'Replace' }).click();
    await expect(page.locator('[data-doc]')).toHaveCount(1);
    // open goes to the existing document
    await addFile(page, file('Contract.pdf'));
    await page.getByRole('dialog').getByRole('button', { name: 'Open' }).click();
    await expect(page).toHaveURL(/#\/doc\//);
  });

  test('a file above the slow limit asks first', async ({ page }) => {
    await page.goto(`/main.html?lang=en&warnmb=1#/folder/${EMPTY}/documents`);
    await addFile(page, file('big.pdf', 2 * 1048576));
    const dlg = page.getByRole('dialog', { name: 'This will take a while' });
    await expect(dlg).toBeVisible();
    await dlg.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.locator('[data-doc]')).toHaveCount(0);
    await addFile(page, file('big.pdf', 2 * 1048576));
    await dlg.getByRole('button', { name: 'Add anyway' }).click();
    await expect(page.locator('[data-doc]', { hasText: 'big' })).toBeVisible();
  });

  test('viewer opens at the page of a source; the editor saves', async ({ page }) => {
    await page.goto('/main.html?lang=en#/doc/01JC8Z3K6Q9X2M4N7R5T0V0401?p=3');
    const pg = page.locator('[data-page="3"]');
    await expect(pg).toHaveClass(/hit/);
    await page.getByRole('button', { name: 'Edit' }).click();
    await page.locator('.cm-content, textarea.fb').first().click();
    await page.keyboard.type('hello ');
    await expect(page.getByText('Unsaved').first()).toBeVisible();
    await page.keyboard.press('ControlOrMeta+s');
    await expect(page.locator('.toast')).toContainText('Saved');
  });

  test('instructions autosave and show the token counter', async ({ page }) => {
    await page.goto(`/main.html?lang=en#/folder/${EMPTY}/instructions`);
    await page.getByRole('button', { name: 'Insert example' }).click();
    await expect(page.getByTestId('instr-counter')).toContainText('/ ~500');
    await expect(page.getByText('Saved').first()).toBeVisible();
  });
});

test.describe('search', () => {
  test('palette: results with highlights, click opens the call at the minute', async ({ page }) => {
    await page.goto('/main.html?lang=en#/');
    await page.keyboard.press('Control+k');
    await page.locator('.pal input').fill('EBITDA margin');
    const first = page.locator('.pal .hit[data-kind=call]').first();
    await expect(first.locator('mark')).not.toHaveCount(0);
    await first.click();
    await expect(page).toHaveURL(/#\/call\/.+\?t=\d+/);
    await expect(page.locator('.ln.hit')).toBeVisible();
  });

  test('keyword-only notice while the semantic index is not ready, with the reason', async ({ page }) => {
    await page.goto('/main.html?lang=en&embedder=building#/search?q=EBITDA');
    await expect(page.getByTestId('mode-notice')).toContainText('building');
    await page.goto('/main.html?lang=en&embedder=missing#/search?q=EBITDA');
    await expect(page.getByTestId('mode-notice')).toContainText('not installed');
    await page.goto('/main.html?lang=en#/search?q=EBITDA');
    await expect(page.getByTestId('mode-notice')).toHaveCount(0);
  });

  test('results page: filters narrow the hits and live in the URL', async ({ page }) => {
    await page.goto('/main.html?lang=en#/search?q=EBITDA');
    const all = Number(/^(\d+)/.exec(await page.getByTestId('search-count').innerText())![1]);
    await page.getByRole('radio', { name: 'Documents' }).click();
    await expect(page).toHaveURL(/kind=doc/);
    await expect(page.locator('[data-group=call]')).toHaveCount(0);
    expect(Number(/^(\d+)/.exec(await page.getByTestId('search-count').innerText())![1])).toBeLessThan(all);
    await page.goto('/main.html?lang=en#/search?q=zzzz');
    await expect(page.getByTestId('search-none')).toBeVisible();
  });
});

test.describe('summary and tags', () => {
  test('stream, done, then tag suggestions: accept adds the tag, reject drops it', async ({ page }) => {
    await page.goto(`/main.html?lang=en#/call/${CALL4}`);
    await page.getByRole('tab', { name: 'Summary' }).click();
    await page.getByRole('button', { name: 'Create summary' }).click();
    await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible();
    const sug = page.getByTestId('tag-suggestions');
    await expect(sug.locator('.sg')).toHaveCount(3, { timeout: 15_000 });
    await sug.getByRole('button', { name: 'Add tag продажи' }).click();
    await sug.getByRole('button', { name: 'Reject tag q4-план' }).click();
    await expect(sug.locator('.sg')).toHaveCount(1);
    await expect(page.getByTestId('tag-editor').first()).toContainText('продажи');
  });

  test('errors surface as readable messages (no key, offline)', async ({ page }) => {
    await page.goto(`/main.html?lang=en&summary=nokey#/call/${CALL1}`);
    await page.getByRole('tab', { name: 'Summary' }).click();
    await page.getByRole('button', { name: 'Create summary' }).click();
    await expect(page.getByTestId('summary-error')).toContainText('OpenRouter key');
    await page.goto(`/main.html?lang=en&summary=offline#/call/${CALL1}`);
    await page.getByRole('tab', { name: 'Summary' }).click();
    await page.getByRole('button', { name: 'Create summary' }).click();
    await expect(page.getByTestId('summary-error')).toContainText('internet');
  });

  test('sources under answers and in the Sources tab open the document at the page', async ({ page }) => {
    await page.goto(`/main.html?lang=en#/call/${CALL1}`);
    await expect(page.getByTestId('answer-sources').first()).toContainText('Folder instructions');
    await page.getByRole('tab', { name: /Sources/ }).click();
    await page.getByTestId('sources-tab').getByRole('link', { name: /Договор-2026.pdf · p. 14/ }).click();
    await expect(page).toHaveURL(/#\/doc\/.+\?p=14/);
  });
});

for (const lang of ['ru', 'en'] as const) {
  test.describe(`tour ${lang}`, () => {
  test.use({ locale: lang === 'ru' ? 'ru-RU' : 'en-US' });
  test(`tour (${lang}): offered after onboarding, 6 steps, demos, done; Esc skips`, async ({ page }) => {
    await page.goto(`/main.html?lang=${lang}&fresh#/onboarding/7`);
    await page.locator('.nav button.primary').click();
    await expect(page.getByTestId('tour-offer')).toBeVisible();
    await page.getByTestId('tour-offer').getByRole('button').first().click();
    const card = page.getByTestId('tour-card');
    for (let i = 1; i <= 6; i++) {
      await expect(page.getByTestId('tour-progress')).toHaveText(`${i} / 6`);
      if (i === 3) { await page.keyboard.press((await mod(page)) + '+Enter'); await expect(page.getByTestId('thumb-answer')).not.toBeEmpty(); }
      if (i === 5) { await page.keyboard.press((await mod(page)) + '+Shift+KeyS'); await expect(page.getByTestId('thumb-shots')).toBeVisible(); }
      await card.locator('.foot button.primary').click();
    }
    await expect(card).toHaveCount(0);
    expect(await page.evaluate(() => location.hash)).toBe('#/');
    // replay from «?», skip with Esc at step 2
    await page.getByTestId('tour-open').click();
    await card.locator('.foot button.primary').click();
    await page.keyboard.press('Escape');
    await expect(card).toHaveCount(0);
  });
  });
}

test.describe('settings', () => {
  test('embedder: licence first, download with progress, then ready', async ({ page }) => {
    await page.goto('/main.html?lang=en&embedder=missing#/settings/knowledge');
    const card = page.getByTestId('embedder-card');
    await expect(card).toContainText('Gemma Terms of Use');
    await card.getByRole('button', { name: /Download/ }).click();
    await expect(card.locator('.pill')).toContainText('Ready', { timeout: 15_000 });
  });

  test('embedder: cancel and failure leave a retry', async ({ page }) => {
    await page.goto('/main.html?lang=en&embedder=fail#/settings/knowledge');
    await page.getByRole('button', { name: /Download/ }).click();
    await expect(page.getByTestId('embedder-model').getByRole('alert')).toBeVisible();
    await expect(page.getByRole('button', { name: /Try again/ })).toBeVisible();
  });

  test('export and import show their result; a bad archive is an error', async ({ page }) => {
    await page.goto('/main.html?lang=en#/settings/storage');
    await page.getByRole('button', { name: 'Export…' }).click();
    await expect(page.getByTestId('transfer-result')).toContainText('Exported 212 files');
    await page.getByRole('button', { name: 'Import…' }).first().click();
    await expect(page.getByTestId('transfer-result')).toContainText('Files copied');
    await page.goto('/main.html?lang=en&import=fail#/settings/storage');
    await page.getByRole('button', { name: 'Import…' }).first().click();
    await expect(page.getByTestId('transfer-result')).toContainText('not a Lowk archive');
  });

  test('pinned budget presets persist', async ({ page }) => {
    await page.goto('/main.html?lang=en#/settings/knowledge');
    await page.getByTestId('pinned-budget').getByRole('radio', { name: '12,000' }).click();
    await expect(page.getByTestId('pinned-budget').getByRole('radio', { name: '12,000' })).toHaveAttribute('aria-checked', 'true');
  });
});
