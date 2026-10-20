import { expect, test, type Page } from '@playwright/test';
import { measureContrast } from './lib/contrast';
import { lintInPage, type Issue } from './lib/lint';
import { shotPath, writeRecord } from './lib/record';
import { BGS, LANGS, MAIN_STATES, OPACITIES, OVERLAY_STATES, SIZES, type Lang } from './lib/states';
import { classify } from './known';

const MAIN_GRID = ['.app', '.tb', '.content'];
const MAIN_GRID_SIZE = ['button.btn', '.gear', '.wm', '.recpill', '.tabrow'];
const MAIN_GRID_PAD = ['.rail', '.nav', '.tabsbar', '.win', '.step', '.chead', '.livebar'];
const MAIN_IGNORE: string[] = [];
const OVERLAY_IGNORE = ['.bar'];

async function prep(page: Page) {
  await page.clock.setFixedTime(new Date('2026-10-20T12:00:00Z'));
}

async function settle(page: Page) {
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(250);
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
          await page.goto(`/main.html?lang=${lang}${st.hash}`);
          await page.waitForSelector('.app, .win');
          await settle(page);
          if (st.act === 'tab-summary') await page.locator('[role=tab]').nth(1).click();
          if (st.act === 'tab-qa') await page.locator('[role=tab]').nth(2).click();
          if (st.act === 'tab-shots') await page.locator('[role=tab]').nth(3).click();
          if (st.act === 'search') {
            await page.keyboard.press('Control+k');
            await page.waitForSelector('input');
          }
          if (st.act === 'recording') {
            await page.locator('.nc button.primary, .nc .btn.primary, header ~ * button.primary').first().click({ timeout: 3000 }).catch(() => {});
            await page.waitForTimeout(700);
          }
          await settle(page);
          await analyse(page, {
            project: info.project.name, group: 'main', id: st.id, title: st.title, lang, size: size.id,
            root: 'body', ignore: MAIN_IGNORE, grid: MAIN_GRID, gridSize: MAIN_GRID_SIZE, gridPad: MAIN_GRID_PAD, mockup: st.mockup, baseline: size.id === 'L' && st.act !== 'recording',
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
