// Reference frames from the visual source of truth (docs/design/lowk-mockups-v3.html) for the gallery.
import { test } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { OUT } from './lib/record';

test.use({ viewport: { width: 1280, height: 800 } });
for (const t of ['ov', 'mn', 'ob'] as const) {
  test(`mockup ${t}`, async ({ page, browserName }) => {
    test.skip(browserName !== 'chromium', 'one reference set is enough');
    const dir = join(OUT, 'mockups');
    mkdirSync(dir, { recursive: true });
    await page.goto('file://' + resolve(process.cwd(), '..', 'docs', 'design', 'lowk-mockups-v3.html'));
    await page.click(`button[data-t=${t}]`);
    await page.waitForTimeout(600);
    await page.locator(`#p-${t} iframe`).screenshot({ path: join(dir, `${t}.png`) });
  });
}
