// One test per previously reported bug that the browser harness can observe.
// `test.fail()` = expected to fail on current main. When the fix lands the test starts passing,
// Playwright reports "expected to fail but passed": the fix then deletes the `test.fail`
// line (and the matching KNOWN entry in known.ts) so the check becomes a permanent regression test.
import { expect, test } from '@playwright/test';
import { measureContrast } from './lib/contrast';
import { lintInPage, dupWords, type Issue } from './lib/lint';

// QA_SHOW_XFAIL=1 runs the F-tests as normal tests so the actual failure message is visible.
const xfail = (why: string) => test.fail(!process.env.QA_SHOW_XFAIL, why);

test.use({ viewport: { width: 1280, height: 800 }, locale: 'en-US', timezoneId: 'UTC' });

test('F1 onboarding fills the window edge-to-edge (no card in a window)', async ({ page }) => {
  await page.goto('/main.html?lang=en#/onboarding/0');
  await page.waitForSelector('.win');
  const r = await page.locator('.win').boundingBox();
  expect(r).toEqual({ x: 0, y: 0, width: 1280, height: 800 });
});

test('F2 header primary button: dot / label / kbd are spaced', async ({ page }) => {
  // G5: no header button on the library home; it lives on every other screen.
  await page.goto('/main.html?lang=ru#/settings/general');
  await page.waitForSelector('.side .nc button.btn');
  const issues = (await page.evaluate(lintInPage, { root: '.side .nc' })) as Issue[];
  expect(issues.filter((i) => i.rule === 'flex-gap')).toEqual([]);
});

test('F2b gaps between inline chips hold across the whole home screen', async ({ page }) => {
  await page.goto('/main.html?lang=en#/');
  await page.locator('.side .nc button.btn.primary').click();
  await page.waitForSelector('.np');
  const issues = (await page.evaluate(lintInPage, { root: '.np' })) as Issue[];
  expect(issues.filter((i) => i.rule === 'flex-gap')).toEqual([]);
});

test('F11 overlay text keeps a legibility scrim over a worst-case busy background at any opacity', async ({ page }) => {
  const bad: string[] = [];
  for (const t of [0, 60, 90]) {
    await page.goto(`/overlay.html?still=1&bg=dark&t=${t}&lang=en`);
    await page.waitForSelector('.frame');
    await page.addStyleTag({ content: '.stage{background:repeating-conic-gradient(#fff 0 25%,#000 0 50%) 0 0/16px 16px !important}.code,.doc,.slide{display:none !important}' });
    await page.waitForTimeout(200);
    const c = await measureContrast(page, '.frame', ['.bar']);
    for (const x of c.filter((x) => ['div.body', 'div.txt', 'b', 'span.tx'].includes(x.sel) && x.ratio < 4.5)) bad.push(`t${t} ${x.sel} "${x.text}" ${x.ratio}:1`);
  }
  expect(bad).toEqual([]);
});

test('F13 toast and move badge stay inside the real 384px overlay window', async ({ page }) => {
  await page.setViewportSize({ width: 600, height: 384 });
  await page.goto('/overlay.html?still=1&bg=dark&t=60&lang=en&bare=1');
  await page.waitForSelector('.frame');
  await page.keyboard.press('s');
  await page.waitForSelector('.toast');
  const toast = await page.locator('.toast').boundingBox();
  expect(toast!.y + toast!.height).toBeLessThanOrEqual(384);
});

test('F13b «Мой ответ» row and hint footer do not overlap and fit the window', async ({ page }) => {
  await page.setViewportSize({ width: 600, height: 384 });
  for (const q of ['', '&mode=interactive']) {
    await page.goto(`/overlay.html?still=1&bg=dark&t=60&lang=ru&bare=1${q}`);
    await page.waitForSelector('.my');
    const my = (await page.locator('.my').boundingBox())!;
    const foot = (await page.locator('.foot').boundingBox())!;
    expect(my.y + my.height, 'my-answer row bottom <= footer top' + q).toBeLessThanOrEqual(foot.y + 0.5);
    expect(foot.y + foot.height, 'footer inside window' + q).toBeLessThanOrEqual(384.5);
  }
});

test('F12 no app list (real backend today): onboarding preselects whole system and home matches it', async ({ page }) => {
  await page.goto('/main.html?lang=en&fresh&src=system&apps=none#/onboarding/2');
  await page.waitForSelector('.win');
  const chosen = await page.locator('.step .opt.on, .step [aria-pressed=true], .step [class*=selected]').first().innerText().catch(() => '');
  await page.locator('.nav button.primary').click();
  await page.waitForTimeout(300);
  await page.evaluate(() => (location.hash = '#/'));
  await page.locator('.side .nc button.btn.primary').click();
  const sources = await page.locator('.np [data-source-value]').evaluateAll((els) => els.map((e) => e.getAttribute('data-source-value')!));
  const onboardingSaysApp = /One app|Одно приложение/i.test(chosen);
  expect(onboardingSaysApp ? /^app:/.test(sources[0]!) : sources[0] === 'system', `onboarding «${chosen.split('\n')[0]}» vs home source «${sources[0]}»`).toBe(true);
});

test('F12 (mock with an app list) Next on the capture step persists the preselected app', async ({ page }) => {
  await page.goto('/main.html?lang=en&fresh&src=system#/onboarding/2');
  await page.waitForSelector('.win');
  await page.locator('.nav button.primary').click();
  await page.waitForTimeout(300);
  await page.evaluate(() => (location.hash = '#/'));
  await page.locator('.side .nc button.btn.primary').click();
  const sources = await page.locator('.np [data-source-value]').evaluateAll((els) => els.map((e) => e.getAttribute('data-source-value')!));
  expect(sources[0]).toMatch(/^app:/);
});

test('F12b onboarding "Skip" on the capture step stores the preselected «one app»; home matches', async ({ page }) => {
  await page.goto('/main.html?lang=en&fresh&src=system#/onboarding/2');
  await page.waitForSelector('.win');
  await page.locator('.nav .skip').click();
  await page.waitForTimeout(300);
  await page.evaluate(() => (location.hash = '#/'));
  await page.locator('.side .nc button.btn.primary').click();
  const sources = await page.locator('.np [data-source-value]').evaluateAll((els) => els.map((e) => e.getAttribute('data-source-value')!));
  expect(sources[0]).toMatch(/^app:/);
});

test('F10 (browser mock) streaming answer applies each delta exactly once', async ({ page }) => {
  await page.setViewportSize({ width: 600, height: 384 });
  await page.goto('/overlay.html?bg=dark&t=60&lang=ru&bare=1');
  const seen: string[] = [];
  for (let i = 0; i < 60; i++) {
    await page.waitForTimeout(100);
    const t = await page.locator('.ans .body').first().innerText().catch(() => '');
    if (t) seen.push(t);
  }
  const dup = seen.flatMap((t) => dupWords(t));
  expect(dup).toEqual([]);
});
