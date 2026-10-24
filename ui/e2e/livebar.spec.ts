// Regression (live check): the live bar cut its labels («Идёт запись. Оверлей показывает субти…», «А…», «Микрофон вы…», «Останов…»)
// and «Остановить» jumped under the cursor because the mic control changed width between its states.
import { expect, test, type Page } from '@playwright/test';

async function openLive(page: Page, lang: 'ru' | 'en') {
  await page.goto(`/main.html?lang=${lang}#/`);
  await page.locator('.side .nc button.btn.primary').click();
  await page.locator('.np button.btn.primary').click();
  await page.waitForSelector('.recpill');
  await page.locator('.recpill').getByRole('button', { name: lang === 'ru' ? 'Открыть' : 'Open' }).click();
  await page.waitForSelector('.livebar');
}

const cut = (page: Page) =>
  page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>('.livebar, .livebar *')]
      .filter((e) => e.scrollWidth > e.clientWidth + 0.5 || e.scrollHeight > e.clientHeight + 0.5)
      .filter((e) => e.clientWidth > 0 && !(e instanceof SVGElement))
      .map((e) => `${e.tagName}.${e.className} ${e.scrollWidth}>${e.clientWidth} "${e.textContent?.trim().slice(0, 30)}"`),
  );

for (const width of [1000, 1280]) {
  for (const lang of ['ru', 'en'] as const) {
    test(`live bar: nothing is cut and the stop button does not move (${width}px, ${lang})`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await openLive(page, lang);
      const mic = page.getByTestId('mic-toggle');
      const stop = page.locator('.livebar').getByRole('button').last();
      const box = async () => ({ mic: await mic.boundingBox(), stop: await stop.boundingBox(), auto: await page.getByTestId('mic-auto').boundingBox() });

      const on = await box();
      expect(await cut(page)).toEqual([]);
      await mic.click();
      await expect(mic).toHaveAttribute('aria-pressed', 'false');
      const off = await box();
      expect(await cut(page)).toEqual([]);
      expect(off.mic!.width).toBe(on.mic!.width);
      expect(off.stop).toEqual(on.stop);
      expect(off.auto).toEqual(on.auto);

      await page.getByTestId('mic-auto').click();
      await expect(page.getByTestId('mic-auto')).toHaveAttribute('aria-pressed', 'true');
      expect(await cut(page)).toEqual([]);
      const auto = await box();
      expect(auto.stop!.x + auto.stop!.width).toBeLessThanOrEqual(width);
      expect(auto.mic!.width).toBe(on.mic!.width);
    });
  }
}
