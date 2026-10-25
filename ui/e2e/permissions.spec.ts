// Settings > Permissions mirrors the onboarding rows with live status (mock IPC; real TCC behaviour is checked on the bundle).
import { expect, test } from '@playwright/test';
import { lintInPage, type Issue } from './lib/lint';

test.use({ viewport: { width: 1280, height: 800 }, timezoneId: 'UTC' });

for (const lang of ['en', 'ru'] as const) {
  const L = lang === 'en'
    ? { allow: 'Allow', open: 'Open settings', granted: 'Allowed', names: ['Microphone', 'System audio', 'Screen recording', 'Input monitoring', 'Accessibility'] }
    : { allow: 'Разрешить', open: 'Открыть настройки', granted: 'Разрешено', names: ['Микрофон', 'Системный звук', 'Запись экрана', 'Мониторинг ввода', 'Универсальный доступ'] };

  test(`permissions page (${lang}, macOS): five rows with status, Allow and Open settings; granting updates the row`, async ({ page }) => {
    await page.goto(`/main.html?lang=${lang}&os=macos#/settings/permissions`);
    const rows = page.locator('[data-testid^=perm-]');
    await expect(rows).toHaveCount(5);
    for (const [i, n] of L.names.entries()) await expect(rows.nth(i)).toContainText(n);
    await expect(rows.nth(0).getByRole('button', { name: L.open, exact: true })).toBeVisible();
    await expect(page.locator('[data-testid^=perm-] .pill')).toHaveCount(0);
    await rows.nth(0).getByRole('button', { name: L.allow, exact: true }).click();
    await expect(page.getByTestId('perm-microphone').locator('.pill')).toBeVisible();
    await page.getByTestId('perm-accessibility').getByRole('button', { name: L.open, exact: true }).click();
    expect(await page.evaluate(() => (globalThis as { __permOpened?: string[] }).__permOpened)).toEqual(['accessibility']);
    await expect(page.locator('.note').filter({ hasText: /Windows/ })).toHaveCount(0);
    const issues = ((await page.evaluate(lintInPage, { root: '.main, main, body' })) as Issue[]).filter((i) => i.rule !== 'grid');
    expect(issues).toEqual([]);
  });
}

test('permissions page (Windows): only the microphone applies, with a note', async ({ page }) => {
  await page.goto('/main.html?lang=en&os=windows#/settings/permissions');
  await expect(page.locator('[data-testid^=perm-]')).toHaveCount(1);
  await expect(page.getByTestId('perm-microphone')).toBeVisible();
  await expect(page.locator('.note')).toContainText('only the microphone');
});

test('permissions page: the same rows as the onboarding step (macOS)', async ({ page }) => {
  await page.goto('/main.html?lang=en&os=macos&fresh&ack&step=3#/');
  await expect(page.locator('[data-testid^=perm-]')).toHaveCount(5);
  const onb = await page.locator('[data-testid^=perm-]').evaluateAll((e) => e.map((x) => x.getAttribute('data-testid')));
  await page.goto('/main.html?lang=en&os=macos#/settings/permissions');
  await expect(page.locator('[data-testid^=perm-]')).toHaveCount(5);
  const set = await page.locator('[data-testid^=perm-]').evaluateAll((e) => e.map((x) => x.getAttribute('data-testid')));
  expect(set).toEqual(onb);
});
