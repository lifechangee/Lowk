// The speech-model download card (Settings > Providers). Before this the install commands had
// no UI at all and the record bind silently played the scripted demo.
import { expect, test } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

async function openCard(page: import('@playwright/test').Page, query = '') {
  await page.goto(`/main.html?lang=en${query}#/settings/providers`);
  const card = page.getByTestId('engine-card');
  await expect(card).toBeVisible();
  return card;
}

test('engine card: not installed says recording cannot start, installs with progress, ends installed', async ({ page }) => {
  const card = await openCard(page);
  await expect(card).toContainText('Not downloaded');
  await expect(card).toContainText('cannot start until the model is installed');
  await card.getByRole('button', { name: /Download/ }).click();
  await expect(card.getByRole('progressbar')).toBeVisible();
  await expect(card.getByRole('button', { name: 'Cancel' })).toBeVisible();
  await expect(card).toContainText('Ready', { timeout: 5000 });
  await expect(card).toContainText('transcribed on this computer');
});

test('engine card: a failed download (checksum) shows a readable reason and a retry button', async ({ page }) => {
  const card = await openCard(page, '&install=fail');
  await card.getByRole('button', { name: /Download/ }).click();
  const alert = card.getByRole('alert');
  await expect(alert).toContainText('integrity check', { timeout: 5000 });
  await expect(card.getByRole('button', { name: /Try again/ })).toBeVisible();
  await expect(card.getByTestId('model-chip')).toHaveAttribute('data-state', 'error');
  await expect(card).toContainText('Error: The file failed');
});

test('engine card: cancel returns to the install button', async ({ page }) => {
  const card = await openCard(page);
  await card.getByRole('button', { name: /Download/ }).click();
  await card.getByRole('button', { name: 'Cancel' }).click();
  await expect(card.getByRole('button', { name: /Download|Try again/ })).toBeVisible({ timeout: 5000 });
});

test('engine card: model installed but the speech component missing says so (no silent demo)', async ({ page }) => {
  const card = await openCard(page, '&runtime=missing');
  await card.getByRole('button', { name: /Download/ }).click();
  await expect(card).toContainText('Ready', { timeout: 5000 });
  await expect(card.getByTestId('engine-runtime-missing')).toContainText('speech component');
  await expect(card).not.toContainText('transcribed on this computer');
});

test('engine card: a worker on the CPU shows a non-blocking notice with the cloud hint (mock IPC)', async ({ page }) => {
  const card = await openCard(page, '&cpu=1');
  await card.getByRole('button', { name: /Download/ }).click();
  await expect(card).toContainText('Ready', { timeout: 5000 });
  const n = card.getByTestId('engine-cpu-notice');
  await expect(n).toContainText('running on the processor, not the graphics card');
  await expect(n).toContainText('cloud engine');
});

test('engine card: no CPU notice when the worker runs on a GPU backend', async ({ page }) => {
  const card = await openCard(page);
  await card.getByRole('button', { name: /Download/ }).click();
  await expect(card).toContainText('Ready', { timeout: 5000 });
  await expect(card.getByTestId('engine-cpu-notice')).toHaveCount(0);
});

test('CPU fallback: a toast appears once the recording started and the worker reported the CPU backend', async ({ page }) => {
  await page.goto('/main.html?lang=ru&cpu=1#/');
  await page.locator('.side .nc button.btn.primary').click();
  await page.getByTestId('start-listen').click();
  await page.locator('.np button.btn.primary').click();
  await page.waitForSelector('.recpill');
  await expect(page.locator('.toast')).toContainText('на процессоре, а не на видеокарте', { timeout: 8000 });
  await expect(page.locator('.toast')).toContainText('облачный движок');
});

// Owner's Windows test: the download was running (started in onboarding) when a new call was started; the app said
// «download the model» and a click started a second download of the same files, which corrupted the first.
test('call cannot start while the model is downloading: the prompt shows the running download, not «download the model»', async ({ page }) => {
  await page.goto('/main.html?lang=en&needed=downloading#/');
  const dlg = page.getByTestId('engine-needed');
  await expect(dlg).toBeVisible();
  await expect(dlg).toHaveAttribute('data-downloading', 'true');
  await expect(dlg).toContainText('already downloading');
  await expect(dlg).not.toContainText('Download it (once)');
  const card = dlg.getByTestId('engine-card');
  await expect(card.getByRole('progressbar')).toBeVisible();
  await expect(card.getByTestId('model-progress')).toContainText(/Downloading [\d,. ]+ of [\d,. ]+ MB/);
  await expect(card.getByTestId('model-chip')).toHaveAttribute('data-state', 'downloading');
  // no button that would start a second download
  await expect(card.getByRole('button', { name: /^Download/ })).toHaveCount(0);
  await expect(dlg.getByRole('button', { name: 'Show in settings' })).toBeVisible();
});

test('call cannot start and nothing is downloading: the prompt offers the download', async ({ page }) => {
  await page.goto('/main.html?lang=en&needed=1#/');
  const dlg = page.getByTestId('engine-needed');
  await expect(dlg).toHaveAttribute('data-downloading', 'false');
  await expect(dlg).toContainText('Download it (once)');
  await expect(dlg.getByTestId('engine-card').getByRole('button', { name: /Download/ })).toBeVisible();
});

test('a download started from the prompt is the same one the settings card shows (one state for the whole app)', async ({ page }) => {
  await page.goto('/main.html?lang=en&needed=1&install=slow#/');
  const dlg = page.getByTestId('engine-needed');
  await dlg.getByTestId('engine-card').getByRole('button', { name: /Download/ }).click();
  await expect(dlg).toHaveAttribute('data-downloading', 'true');
  await expect(dlg.getByTestId('engine-card').getByRole('progressbar')).toBeVisible();
  await expect(dlg.getByTestId('engine-card').getByRole('button', { name: /^Download/ })).toHaveCount(0);
});

test('a damaged part is re-downloaded with a transient notice, the bar never goes down, no terminal error', async ({ page }) => {
  await page.goto('/main.html?lang=en&install=repair#/settings/providers');
  const card = page.getByTestId('engine-card');
  await card.getByRole('button', { name: /Download/ }).click();
  const pcts: number[] = [];
  let sawRetry = false;
  let sawRepair = false;
  const t0 = Date.now();
  for (;;) {
    const snap = await page.evaluate(() => {
      const c = document.querySelector('[data-testid=engine-card]')!;
      const w = c.querySelector('[role=progressbar] i') as HTMLElement | null;
      return { state: c.querySelector('[data-testid=model-chip]')?.getAttribute('data-state') ?? '', note: c.querySelector('[data-testid=model-note]')?.textContent ?? '', w: w ? parseFloat(w.style.width) : NaN, err: c.querySelectorAll('[data-testid=model-error]').length };
    });
    if (snap.state === 'ready' || Date.now() - t0 > 15_000) break;
    if (/reconnecting/i.test(snap.note)) sawRetry = true;
    if (/damaged part/.test(snap.note)) sawRepair = true;
    if (!Number.isNaN(snap.w)) pcts.push(snap.w);
    expect(snap.err, 'no terminal error while repairing').toBe(0);
    await page.waitForTimeout(40);
  }
  expect(sawRetry, 'reconnecting notice').toBe(true);
  expect(sawRepair, 'damaged-part notice').toBe(true);
  for (let i = 1; i < pcts.length; i++) expect(pcts[i]!, pcts.join(',')).toBeGreaterThanOrEqual(pcts[i - 1]!);
  await expect(card.getByTestId('model-chip')).toHaveAttribute('data-state', 'ready');
});
