import { expect, test, type Page } from '@playwright/test';
import { measureContrast } from './lib/contrast';
import { lintInPage, type Issue } from './lib/lint';
import { shotPath, writeRecord } from './lib/record';
import { BGS, LANGS, MAIN_STATES, OPACITIES, OVERLAY_STATES, SIZES, type Lang } from './lib/states';
import { classify } from './known';

const MAIN_GRID = ['.app', '.tb', '.content'];
const MAIN_GRID_SIZE = ['button.btn', '.recpill', '.tabrow'];
const MAIN_GRID_PAD = ['.rail', '.nav', '.tabsbar', '.win', '.step', '.chead', '.livebar'];
const MAIN_IGNORE: string[] = [];
const OVERLAY_IGNORE = ['.bar'];

/** The mock picks Meta binds on a Mac user agent (webkit) and Ctrl elsewhere (chromium reports Windows). */
const mod = (page: Page) => page.evaluate(() => (/Macintosh|Mac OS/.test(navigator.userAgent) ? 'Meta' : 'Control'));

async function prep(page: Page) {
  await page.clock.setFixedTime(new Date('2026-10-20T12:00:00Z'));
}

async function settle(page: Page) {
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(250);
}


const BIG = { name: 'Годовой отчёт.pdf', mimeType: 'application/pdf', buffer: Buffer.alloc(2 * 1048576, 1) };

async function act(page: Page, a: string | undefined) {
  if (!a) return;
  const newCall = () => page.locator('.side .nc button.btn.primary').click();
  if (a === 'tpl-edit') { await page.locator('[data-tpl=complement] .main').click(); await page.waitForSelector('[data-testid=tpl-editor]'); }
  if (a === 'tpl-new') { await page.locator('.foot button.primary').click(); await page.locator('[data-testid=tpl-editor] button.primary').click(); await page.waitForSelector('[data-testid=tpl-editor] [role=alert]'); }
  if (a === 'tpl-delete') { await page.locator('[data-tpl=objection] .main').click(); await page.locator('[data-testid=tpl-editor] button.danger').click(); await page.waitForSelector('.modal'); }
  if (a === 'tab-summary') await page.getByRole('tab').nth(1).click();
  if (a === 'tab-qa') await page.getByRole('tab').nth(2).click();
  if (a === 'tab-shots') await page.getByRole('tab').nth(3).click();
  if (a === 'tab-sources') await page.getByRole('tab').nth(4).click();
  if (a === 'search') { await page.keyboard.press('Control+k'); await page.waitForSelector('.pal input'); }
  if (a.startsWith('palette-q:')) {
    await page.keyboard.press('Control+k');
    await page.locator('.pal input').fill(a.slice(10));
    await page.waitForSelector('.pal .hit, [data-testid=palette-empty]');
  }
  if (a === 'move-menu') { await page.locator('.tbl .tr').first().click({ button: 'right' }); await page.waitForSelector('[role=menu]'); }
  if (a === 'drag') {
    const row = page.locator('.tbl .tr', { hasText: /Быстрый|подрядчиком/ }).first();
    await row.scrollIntoViewIfNeeded();
    const from = (await row.boundingBox())!;
    await page.mouse.move(from.x + 40, from.y + from.height / 2);
    await page.mouse.down();
    await page.mouse.move(from.x + 60, from.y + from.height / 2 + 6, { steps: 3 });
    const to = (await page.locator('.side .f[data-drop]').first().boundingBox())!;
    await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 6 });
    await page.waitForSelector('.dropping');
  }
  if (a === 'new-call-popover') { await newCall(); await page.waitForSelector('.np'); }
  if (a === 'source-picker' || a === 'source-other') {
    await newCall();
    await page.locator('.np .trig').click();
    await page.waitForSelector('[role=listbox]');
    await page.addStyleTag({ content: '.np > .btn { visibility: hidden }' }); // the open list covers the Start button
    if (a === 'source-other') {
      await page.getByRole('option', { name: /Other…|Другое…/ }).click();
      await page.waitForSelector('.modal .list button');
    }
  }
  if (a === 'recording') { await newCall(); await page.locator('.np button.btn.primary').click(); await page.waitForTimeout(700); }
  if (a === 'slow-warning') { const ch = page.waitForEvent('filechooser'); await page.getByRole('button', { name: /Add document|Добавить документ/ }).click(); (await ch).setFiles(BIG); await page.waitForSelector('.modal'); }
  if (a === 'doc-menu') { await page.locator('[data-doc] [aria-haspopup=menu]').first().click(); await page.waitForSelector('[role=menu]'); }
  if (a === 'doc-edit') { await page.getByRole('button', { name: /Edit|Править/ }).click(); await page.waitForSelector('.cm-editor, textarea.fb'); }
  if (a === 'call-menu') { await page.getByTestId('call-menu').click(); await page.waitForSelector('[role=menu]'); }
  if (a === 'call-props') { await page.getByTestId('call-menu').click(); await page.getByRole('menuitem').first().click(); await page.waitForSelector('[data-testid=call-props]'); }
  if (a === 'summary-stream') { await page.getByRole('tab').nth(1).click(); await page.getByRole('button', { name: /Create summary|Создать саммари/ }).click(); await page.waitForSelector('[data-testid=md] ul'); await page.waitForTimeout(500); }
  if (a === 'summary-done') { await page.getByRole('tab').nth(1).click(); await page.getByRole('button', { name: /Create summary|Создать саммари/ }).click(); await page.getByRole('button', { name: /Refresh summary|Обновить саммари/ }).waitFor({ timeout: 15000 }); }
  if (a === 'summary-error') { await page.getByRole('tab').nth(1).click(); await page.getByRole('button', { name: /Create summary|Создать саммари/ }).click(); await page.waitForSelector('[data-testid=summary-error]'); }
  if (a === 'embedder-install' || a === 'embedder-fail') {
    await page.getByRole('button', { name: /Download|Скачать/ }).click();
    await page.waitForSelector(a === 'embedder-install' ? '[data-testid=embedder-model] [role=progressbar]' : '[data-testid=embedder-model] [role=alert]');
    await page.waitForTimeout(a === 'embedder-install' ? 500 : 100);
  }
  if (a === 'export-vault') { await page.getByRole('button', { name: /Export…|Экспорт…/ }).click(); await page.waitForSelector('[data-testid=transfer-result]'); }
  if (a === 'import-vault') { await page.getByRole('button', { name: /Import…|Импорт…/ }).first().click(); await page.waitForSelector('[data-testid=transfer-result]'); }
  if (a === 'key-test') {
    await page.locator('input[type=password]').first().fill('sk-or-test');
    await page.getByRole('button', { name: /^(Save|Сохранить)$/ }).first().click();
    await page.getByRole('button', { name: /^(Check|Проверить)$/ }).first().click();
    await page.waitForSelector('.msg');
  }
  if (a === 'tour-offer') { await page.locator('.nav button.primary').click(); await page.waitForSelector('[data-testid=tour-offer]'); }
  if (a.startsWith('tour-') && /^tour-\d$/.test(a)) { await openTour(page, Number(a.slice(5))); }
  if (a === 'tour-try-ask') { await openTour(page, 2); await page.keyboard.press((await mod(page)) + '+Enter'); await page.waitForSelector('[data-testid=thumb-answer]'); await page.waitForTimeout(1500); }
  if (a === 'tour-try-hold') { await openTour(page, 3); await page.keyboard.down('Alt'); await page.mouse.move(700, 400); await page.mouse.move(760, 420); await page.waitForSelector('[data-moving]'); }
  if (a === 'tour-try-shot') { await openTour(page, 4); await page.keyboard.press((await mod(page)) + '+Shift+KeyS'); await page.waitForSelector('[data-testid=thumb-shots]'); }
}

async function openTour(page: Page, step: number) {
  await page.getByTestId('tour-open').click();
  await page.waitForSelector('[data-testid=tour-card]');
  for (let i = 0; i < step; i++) await page.locator('[data-testid=tour-card] .foot button.primary').click();
}

async function analyse(page: Page, o: { project: string; group: string; id: string; title: string; lang: Lang; size: string; root: string; ignore: string[]; grid: string[]; gridSize?: string[]; gridPad?: string[]; mockup: 'ov' | 'mn' | 'ob'; mask?: string[]; baseline: boolean }) {
  const key = `${o.group}/${o.id}`;
  const sp = shotPath(o.project, `${o.group}-${o.id}-${o.lang}-${o.size}`);
  await page.screenshot({ path: sp.abs });
  const issues = (await page.evaluate(lintInPage, { root: o.root, ignore: o.ignore, grid: o.grid, gridSize: o.gridSize, gridPad: o.gridPad })) as Issue[];
  const contrast = await measureContrast(page, o.root, o.ignore);
  writeRecord({
    id: `${o.group}-${o.id}-${o.lang}-${o.size}`,
    group: o.group,
    project: o.project,
    title: o.title,
    lang: o.lang,
    size: o.size,
    mockup: o.mockup,
    shot: sp.rel,
    issues: issues.map((i) => ({ ...i, fid: classify(i.rule, i.sel, key) })),
    contrast: contrast.map((c) => ({ ...c, fid: classify('contrast', c.sel, key) })),
  });
  if (o.baseline) {
    await expect(page).toHaveScreenshot(`${o.group}-${o.id}-${o.lang}-${o.size}.png`, { mask: (o.mask ?? []).map((m) => page.locator(m)) });
  }
  const unknown = issues.filter((i) => !classify(i.rule, i.sel, key));
  // < 3:1 is unreadable (hard fail); 3..4.5 is reported as a warning in the gallery.
  const unknownC = contrast.filter((c) => c.ratio < 3 && !classify('contrast', c.sel, key));
  expect.soft(unknown, `layout lint (${key})`).toEqual([]);
  expect.soft(unknownC, `contrast (${key})`).toEqual([]);
}

for (const lang of LANGS) {
  for (const size of SIZES) {
    test.describe(`main ${lang} ${size.id}`, () => {
      test.use({ viewport: { width: size.w, height: size.h }, locale: lang === 'ru' ? 'ru-RU' : 'en-US', timezoneId: 'UTC' });
      for (const st of MAIN_STATES) {
        test(`${st.id}`, async ({ page }, info) => {
          await prep(page);
          await page.goto(`/main.html?lang=${lang}${st.q ?? ''}${st.hash}`);
          await page.waitForSelector('.app, .win');
          await settle(page);
          await act(page, st.act);
          await settle(page);
          await analyse(page, {
            project: info.project.name, group: 'main', id: st.id, title: st.title, lang, size: size.id,
            root: 'body', ignore: MAIN_IGNORE, grid: MAIN_GRID, gridSize: MAIN_GRID_SIZE, gridPad: MAIN_GRID_PAD, mockup: st.mockup, baseline: size.id === 'L' && !['recording', 'summary-stream', 'tour-try-ask', 'tour-try-hold'].includes(st.act ?? ''),
          });
        });
      }
    });
  }
}

// Overlay: size L = test stage 1280x800 (harness chrome, backgrounds); size S = "bare": the real
// 600x384 overlay window (pill: 140x22), so anything outside the viewport is genuinely clipped.
const OV_SIZES = [{ id: 'L', w: 1280, h: 800, bare: false }, { id: 'S', w: 600, h: 384, bare: true }] as const;
for (const lang of LANGS) {
  for (const size of OV_SIZES) {
    test.describe(`overlay ${lang} ${size.id}`, () => {
      test.use({ viewport: { width: size.w, height: size.h }, locale: lang === 'ru' ? 'ru-RU' : 'en-US', timezoneId: 'UTC' });
      for (const st of OVERLAY_STATES) {
        const ops = st.full ? OPACITIES : ([60] as const);
        for (const bg of st.bgs ?? BGS) {
          for (const t of ops) {
            test(`${st.id} ${bg} t${t}`, async ({ page }, info) => {
              await prep(page);
              if (size.bare && st.pill) await page.setViewportSize({ width: 140, height: 22 });
              const q = new URLSearchParams({ ...st.q, bg, t: String(t), lang, ...(size.bare ? { bare: '1' } : {}) });
              await page.goto(`/overlay.html?${q}`);
              await page.waitForSelector('.frame');
              await settle(page);
              for (const k of st.keys ?? []) await page.keyboard.press(k);
              if (st.keys) await page.waitForTimeout(150);
              await analyse(page, {
                project: info.project.name, group: 'overlay', id: `${st.id}-${bg}-t${t}`, title: `${st.title} / ${bg} / opacity ${t}`, lang, size: size.id,
                root: '.frame', ignore: OVERLAY_IGNORE, grid: [], gridPad: ['.ov', '.head', '.foot', '.ans', '.sub'], mockup: 'ov', mask: ['.rec .lbl'], baseline: size.id === 'L',
              });
            });
          }
        }
      }
    });
  }
}
