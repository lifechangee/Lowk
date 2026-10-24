// Overlay start-up budget: time from navigation start to the first rendered content, against the PRODUCTION
// bundle (dist/, served locally) with a Tauri bridge whose every command answers only after 3 s. First paint
// must not wait for the backend (owner report: frame visible, content very late).
import { expect, test } from '@playwright/test';
import { execSync } from 'node:child_process';
import { createServer, type Server } from 'node:http';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { extname, join, normalize } from 'node:path';
import type { AddressInfo } from 'node:net';

const DIST = join(process.cwd(), 'dist');
const TYPES: Record<string, string> = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml' };
export const FIRST_CONTENT_BUDGET_MS = 150;

let server: Server;
let base = '';

test.beforeAll(async () => {
  if (!existsSync(join(DIST, 'overlay.html'))) execSync('pnpm build', { stdio: 'inherit' });
  server = createServer((req, res) => {
    const p = normalize(decodeURIComponent((req.url ?? '/').split('?')[0]!.split('#')[0]!)).replace(/^(\.\.[/\\])+/, '');
    const f = join(DIST, p);
    if (!f.startsWith(DIST) || !existsSync(f) || !statSync(f).isFile()) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'content-type': TYPES[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
  });
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});
test.afterAll(() => server.close());

const SLOW_BACKEND = `
  window.__TAURI_INTERNALS__ = {
    invoke: () => new Promise((r) => setTimeout(() => r(null), 3000)),
    transformCallback: () => 1,
    unregisterCallback: () => {},
    metadata: { currentWindow: { label: 'overlay' }, currentWebview: { label: 'overlay', windowLabel: 'overlay' } },
  };
  window.__fc = -1;
  new MutationObserver((_, o) => {
    if (document.querySelector('.ov [data-blk]')) {
      o.disconnect();
      requestAnimationFrame(() => requestAnimationFrame(() => (window.__fc = performance.now())));
    }
  }).observe(document, { childList: true, subtree: true });
`;

async function firstContent(page: import('@playwright/test').Page, url: string): Promise<number> {
  await page.addInitScript(SLOW_BACKEND);
  await page.goto(url, { waitUntil: 'commit' });
  await page.waitForFunction(() => (window as unknown as { __fc: number }).__fc > 0, null, { timeout: 2500 });
  return page.evaluate(() => (window as unknown as { __fc: number }).__fc);
}

test.use({ viewport: { width: 600, height: 384 }, locale: 'en-US' });

test('overlay start-up: first content <= 150 ms cold and warm, without waiting for the backend', async ({ page, browserName }) => {
  const cold = await firstContent(page, `${base}/overlay.html`);
  // the backend is still "thinking" (3 s): content is already there
  await expect(page.locator('.ov [data-blk]').first()).toBeVisible();
  await page.reload({ waitUntil: 'commit' });
  await page.waitForFunction(() => (window as unknown as { __fc: number }).__fc > 0, null, { timeout: 2500 });
  const warm = await page.evaluate(() => (window as unknown as { __fc: number }).__fc);
  console.log(`overlay first content: cold ${cold.toFixed(0)} ms, warm ${warm.toFixed(0)} ms`);
  // WebKit's very first navigation also pays its content-process spawn (~100 ms headless): 250 ms there.
  expect(cold).toBeLessThanOrEqual(browserName === 'webkit' ? 250 : FIRST_CONTENT_BUDGET_MS);
  expect(warm).toBeLessThanOrEqual(FIRST_CONTENT_BUDGET_MS);
});

test('overlay start-up: the first-content mark is set and no command was awaited before it', async ({ page }) => {
  await firstContent(page, `${base}/overlay.html`);
  const m = await page.evaluate(() => performance.getEntriesByName('overlay-first-content').length);
  expect(m).toBe(1);
});
