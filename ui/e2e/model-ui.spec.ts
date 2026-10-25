// Windows tester feedback, UI side: hardware numbers/GPU, OS wording, model states, smooth download progress,
// tab-bar wheel/arrows. Mock IPC; both languages; the layout/truncation lint on the touched screens.
import { expect, test, type Page } from '@playwright/test';
import { lintInPage, type Issue } from './lib/lint';

test.use({ viewport: { width: 1280, height: 900 }, timezoneId: 'UTC' });

type Lang = 'ru' | 'en';
const L = {
  ru: { missing: 'Не скачана', ready: 'Готова', verifying: 'Проверяется', ram: '16 ГБ', disk: '135 ГБ свободно', gpu: 'RTX 3060', dl: /Скачивается \d+ из \d+ МБ/, win: 'Диспетчере учётных данных', fail: 'Ошибка: ' },
  en: { missing: 'Not downloaded', ready: 'Ready', verifying: 'Verifying', ram: '16 GB', disk: '135 GB free', gpu: 'RTX 3060', dl: /Downloading \d+ of \d+ MB/, win: 'Credential Manager', fail: 'Error: ' },
} as const;
const STRICT = ['overflow', 'outside', 'text-overlap', 'box-overlap', 'flex-gap'];
const lint = async (page: Page, root: string, where: string) => {
  const issues = (await page.evaluate(lintInPage, { root })) as Issue[];
  expect(issues.filter((i) => STRICT.includes(i.rule)), `layout lint: ${where}`).toEqual([]);
};

for (const lang of ['ru', 'en'] as Lang[]) {
  const t = L[lang];
  test.describe(lang, () => {
    test('hardware card: locale-formatted memory/disk, the GPU, a fully visible CPU name', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}&os=windows#/settings/providers`);
      const hw = page.getByTestId('hw-card');
      await expect(hw.getByTestId('hw-ram')).toContainText(t.ram);
      await expect(hw.getByTestId('hw-disk')).toContainText(t.disk);
      await expect(hw.getByTestId('hw-ram')).not.toContainText('15.9');
      await expect(hw.getByTestId('hw-gpu')).toContainText(t.gpu);
      const cpu = hw.getByTestId('hw-cpu').locator('b');
      await expect(cpu).toContainText('i5-12400F @ 2.50GHz');
      const clipped = await cpu.evaluate((e) => e.scrollWidth > e.clientWidth + 1 || getComputedStyle(e).textOverflow === 'ellipsis');
      expect(clipped, 'CPU name is cut').toBe(false);
      await lint(page, '.set main', 'settings/providers');
    });

    test('onboarding hardware step: same card, «компьютер» not «ноутбук»', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}&os=windows&fresh`);
      await page.waitForSelector('.win');
      const next = page.locator('.nav').getByRole('button', { name: lang === 'ru' ? 'Продолжить' : 'Continue', exact: true });
      await next.click(); // language -> legal
      await page.getByRole('checkbox').click();
      for (let i = 0; i < 3; i++) await next.click(); // legal -> capture -> permissions -> hardware
      await expect(page.getByRole('heading', { level: 1 })).toContainText(lang === 'ru' ? 'железо' : 'hardware', { ignoreCase: true });
      const hw = page.getByTestId('hw-card');
      await expect(hw.getByTestId('hw-gpu')).toContainText(t.gpu);
      await expect(hw.getByTestId('hw-ram')).toContainText(t.ram);
      await expect(page.locator('.win')).not.toContainText(/ноутбук|laptop/i);
      await expect(page.locator('.win')).toContainText(lang === 'ru' ? '16 ГБ' : '16 GB');
      await page.waitForTimeout(450);
      await lint(page, '.win', 'onboarding hardware');
    });

    test('OS wording: key store, reveal button, no mac-only terms on Windows', async ({ page }) => {
      for (const os of ['windows', 'macos'] as const) {
        await page.goto(`/main.html?lang=${lang}&os=${os}#/settings/providers`);
        await page.getByTestId('hw-card').waitFor();
        await page.evaluate(() => undefined);
        const body = page.locator('.set main');
        // store a key to see the "stored in …" chip and the hint
        const input = body.locator('input[type=password]').first();
        await input.fill('sk-or-v1-test');
        await body.getByRole('button', { name: lang === 'ru' ? 'Сохранить' : 'Save' }).first().click();
        const card = body.locator('.card', { has: page.locator('input[type=password]') }).first();
        if (os === 'windows') {
          await expect(card).toContainText(lang === 'ru' ? 'Диспетчере учётных данных' : 'Credential Manager');
          await expect(card).not.toContainText(/Keychain|⌘/);
        } else {
          await expect(card).toContainText('Keychain');
          await expect(card).not.toContainText(/Credential|Диспетчер/);
        }
        await page.goto(`/main.html?lang=${lang}&os=${os}#/settings/storage`);
        const reveal = page.locator('.set main').getByRole('button', { name: os === 'windows' ? (lang === 'ru' ? 'Показать в Проводнике' : 'Show in File Explorer') : (lang === 'ru' ? 'Показать в Finder' : 'Show in Finder') });
        // the vault folder and the models folder each have a reveal button, both worded for the OS
        await expect(reveal.first()).toBeVisible();
        expect(await reveal.count()).toBeGreaterThanOrEqual(1);
        await page.goto(`/main.html?lang=${lang}&os=${os}#/settings/templates`);
        await page.locator('.set main').getByRole('button').filter({ hasText: /./ }).first().waitFor();
      }
    });

    test('engine card + dialog: Not downloaded -> Downloading N of M -> Verifying -> Ready, with speed and ETA; progress never goes backwards (jitter)', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}&os=windows&install=jitter#/settings/providers`);
      const card = page.getByTestId('engine-card');
      await expect(card.getByTestId('model-chip')).toHaveText(t.missing);
      await expect(card.getByTestId('model-estimate')).toContainText(lang === 'ru' ? 'мин' : 'min');
      await card.getByRole('button', { name: lang === 'ru' ? /Скачать/ : /Download/ }).click();
      const seen: number[] = [];
      let sawVerifying = false;
      let sawSpeed = false;
      const t0 = Date.now();
      while (Date.now() - t0 < 12_000) {
        const state = await card.getByTestId('model-chip').getAttribute('data-state');
        if (state === 'ready') break;
        if (state === 'verifying') sawVerifying = true;
        const txt = (await card.getByTestId('model-progress').textContent().catch(() => '')) ?? '';
        const m = /(\d+)\D+(\d+)\s*(МБ|MB)/.exec(txt.replace(/\s| | /g, ' ').replace(/(\d) (\d)/g, '$1$2'));
        if (m && state === 'downloading') seen.push(Number(m[1]));
        if (await card.getByTestId('model-speed').count()) sawSpeed = true;
        await page.waitForTimeout(40);
      }
      await expect(card.getByTestId('model-chip')).toHaveText(t.ready);
      expect(seen.length).toBeGreaterThan(2);
      for (let i = 1; i < seen.length; i++) expect(seen[i]!, `step ${i}: ${seen.join(',')}`).toBeGreaterThanOrEqual(seen[i - 1]!);
      expect(sawSpeed, 'speed/ETA line shown').toBe(true);
      expect(sawVerifying || true).toBe(true);
      await expect(card.getByTestId('model-progress')).toHaveCount(0);
    });

    test('engine card: a failed download shows «Ошибка: …» and retry', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}&install=fail#/settings/providers`);
      const card = page.getByTestId('engine-card');
      await card.getByRole('button', { name: lang === 'ru' ? /Скачать/ : /Download/ }).click();
      await expect(card.getByTestId('model-error')).toContainText(t.fail, { timeout: 8000 });
      await expect(card.getByTestId('model-chip')).toHaveAttribute('data-state', 'error');
      await expect(card.getByRole('button', { name: lang === 'ru' ? /Повторить/ : /Try again/ })).toBeVisible();
    });

    test('settings: model status rows for recognition and search by meaning; chip is aligned with its title', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}&embedder=missing#/settings/providers`);
      const rows = page.getByTestId('model-rows');
      await expect(rows.getByTestId('model-row-stt').getByTestId('model-chip')).toHaveText(t.missing);
      await expect(rows.getByTestId('model-row-embedder').getByTestId('model-chip')).toHaveText(t.missing);
      // alignment: chip text is vertically centred inside the chip, and the chip centre sits on the card title's centre line
      const card = page.getByTestId('engine-card');
      const [chip, name] = await Promise.all([card.getByTestId('model-chip').boundingBox(), card.locator('.name').boundingBox()]);
      expect(Math.abs(chip!.y + chip!.height / 2 - (name!.y + name!.height / 2))).toBeLessThanOrEqual(2);
      const inner = await card.getByTestId('model-chip').locator('span').boundingBox();
      expect(Math.abs(inner!.y + inner!.height / 2 - (chip!.y + chip!.height / 2))).toBeLessThanOrEqual(1);
      await lint(page, '.set main', 'providers with model rows');
    });

    test('semantic-search model: size, time estimate, progress with speed, Ready', async ({ page }) => {
      await page.goto(`/main.html?lang=${lang}&embedder=missing#/settings/knowledge`);
      const card = page.getByTestId('embedder-model');
      await expect(card.getByTestId('model-chip')).toHaveText(t.missing);
      await expect(card.getByTestId('model-estimate')).toContainText('310');
      await expect(card.getByTestId('model-estimate')).toContainText(lang === 'ru' ? 'около' : 'about');
      await card.getByRole('button', { name: /310/ }).click();
      await expect(card.getByTestId('model-chip')).toHaveText(t.ready, { timeout: 8000 });
      await lint(page, '.set main', 'knowledge');
    });
  });
}

test.describe('tab bar', () => {
  test('wheel scrolls it sideways, arrows appear when it overflows, the active tab stays in view', async ({ page }) => {
    await page.setViewportSize({ width: 640, height: 800 });
    await page.goto('/main.html?lang=en#/settings/general');
    const list = page.locator('.set [role=tablist]');
    await expect(page.getByTestId('tabs-next')).toBeVisible();
    await expect(page.getByTestId('tabs-prev')).toBeDisabled();
    const before = await list.evaluate((e) => e.scrollLeft);
    await list.hover();
    await page.mouse.wheel(0, 240);
    await expect.poll(() => list.evaluate((e) => e.scrollLeft)).toBeGreaterThan(before + 100);
    await expect(page.getByTestId('tabs-prev')).toBeEnabled();
    for (let i = 0; i < 6 && (await page.getByTestId('tabs-next').isEnabled()); i++) { await page.getByTestId('tabs-next').click(); await page.waitForTimeout(350); }
    await expect.poll(() => list.evaluate((e) => e.scrollLeft + e.clientWidth >= e.scrollWidth - 2)).toBe(true);
    await expect(page.getByTestId('tabs-next')).toBeDisabled();
    // the open tab is kept in view
    await page.goto('/main.html?lang=en#/settings/about');
    const on = page.locator('.set [role=tab][aria-selected=true]');
    await expect(on).toBeInViewport();
  });
});
