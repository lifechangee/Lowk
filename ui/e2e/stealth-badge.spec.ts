// Stealth self-test badge in the main window (mock IPC with a fake StealthGuard state; the real probe runs in crates/lowk-stealth tests).
import { expect, test } from '@playwright/test';
import { lintInPage, type Issue } from './lib/lint';

test.use({ viewport: { width: 1280, height: 800 }, timezoneId: 'UTC' });

const badge = (p: import('@playwright/test').Page) => p.getByTestId('stealth-badge');

test('badge is green «hidden from screen share» when the guard is verified; the popover explains the check', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await expect(badge(page)).toHaveAttribute('data-level', 'safe');
  await expect(badge(page)).toContainText('hidden from screen share');
  await badge(page).click();
  const pop = page.getByTestId('stealth-popover');
  await expect(pop).toContainText('excluded from screen capture');
  await expect(pop).toContainText('main Lowk window is not hidden');
  await expect(pop).not.toContainText('What to do.');
  const issues = ((await page.evaluate(lintInPage, { root: '[data-testid=stealth-popover]' })) as Issue[]).filter((i) => i.rule !== 'grid');
  expect(issues).toEqual([]);
  await page.keyboard.press('Escape');
  await expect(pop).toHaveCount(0);
});

test('badge is red «may be visible» when protection is degraded and tells what to do', async ({ page }) => {
  await page.goto('/main.html?lang=en&stealth=degraded#/');
  await expect(badge(page)).toHaveAttribute('data-level', 'risk');
  await expect(badge(page)).toContainText('may be visible');
  await badge(page).click();
  const pop = page.getByTestId('stealth-popover');
  await expect(pop).toContainText('not confirmed');
  await expect(pop).toContainText('What to do.');
  await pop.getByRole('button', { name: 'Check now' }).click();
  await expect(page.getByTestId('stealth-report')).toContainText('appeared in the capture');
  await expect(badge(page)).toHaveAttribute('data-level', 'risk');
});

test('a re-check that now passes turns the badge green (status event)', async ({ page }) => {
  await page.goto('/main.html?lang=en&stealth=flip#/');
  await expect(badge(page)).toHaveAttribute('data-level', 'risk');
  await badge(page).click();
  await page.getByTestId('stealth-popover').getByRole('button', { name: 'Check now' }).click();
  await expect(badge(page)).toHaveAttribute('data-level', 'safe');
});

test('unavailable protection reads as «may be visible» (never green)', async ({ page }) => {
  await page.goto('/main.html?lang=en&stealth=unavailable#/');
  await expect(badge(page)).toHaveAttribute('data-level', 'risk');
});

test('Russian labels and Windows variant (no permissions shortcut)', async ({ page }) => {
  await page.goto('/main.html?lang=ru&os=windows#/');
  await expect(badge(page)).toContainText('скрыт от демонстрации');
  await badge(page).click();
  await expect(page.getByTestId('stealth-popover').getByRole('button', { name: 'Разрешения' })).toHaveCount(0);
  await page.goto('/main.html?lang=ru&os=macos&stealth=degraded#/');
  await expect(badge(page)).toContainText('может быть виден');
});
