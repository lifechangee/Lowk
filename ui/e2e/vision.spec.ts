// Screenshots with an Ask: the overlay says how many went out and which model answered, and every refusal
// (text-only model, local-only folder, too large) has a clear RU/EN message.
import { expect, test } from '@playwright/test';

test.use({ viewport: { width: 600, height: 384 } });

type Ov = { applyLlm: (e: unknown) => void };

const open = async (page: import('@playwright/test').Page, lang: 'ru' | 'en') => {
  await page.goto(`/overlay.html?still=1&t=60&lang=${lang}&bare=1&bg=dark&expose=1`);
  await page.waitForSelector('.ans .body');
};
const send = (page: import('@playwright/test').Page, evs: unknown[]) =>
  page.evaluate((list) => {
    const s = (window as unknown as { __ovStore: Ov }).__ovStore;
    for (const e of list) s.applyLlm(e);
  }, evs);

const start = { type: 'start', req_id: 'v1', qa_id: 'q1' };

for (const lang of ['ru', 'en'] as const) {
  test(`overlay shows the screenshot count and the answering model (${lang})`, async ({ page }) => {
    await open(page, lang);
    await send(page, [start, { type: 'meta', req_id: 'v1', model: 'anthropic/claude-haiku-5.5', images: 2 }, { type: 'delta', req_id: 'v1', seq: 0, text: 'B' }]);
    await expect(page.getByTestId('ans-images')).toHaveText(lang === 'ru' ? 'Отправлено скриншотов: 2' : 'Screenshots sent: 2');
    await expect(page.getByTestId('ans-model')).toHaveText('claude-haiku-5.5');
  });

  const errors: Array<[string, RegExp, RegExp]> = [
    ['no_vision', /не умеет читать изображения/, /cannot read images/],
    ['image_local_only', /не уходят в облако/, /never go to the cloud/],
    ['image_too_large', /слишком большие/, /too large/],
  ];
  for (const [code, ru, en] of errors) {
    test(`refusal ${code} is explained (${lang})`, async ({ page }) => {
      await open(page, lang);
      await send(page, [start, { type: 'error', req_id: 'v1', error: { code, params: {}, detail: null } }]);
      await expect(page.locator('.ans .err')).toHaveText(lang === 'ru' ? ru : en);
      await expect(page.getByTestId('ans-images')).toHaveCount(0);
    });
  }
}
