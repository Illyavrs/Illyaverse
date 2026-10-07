import { test, expect, type Page } from '@playwright/test';

// The docs response and its Markdown are real; only the reader's guest account is mocked.
async function guestReader(page: Page, locale = 'en', mode = 'dark') {
  await page.context().addCookies([{ name: 'illya_locale', value: locale, url: test.info().project.use.baseURL! }]);
  await page.addInitScript(({ locale, mode }) => {
    localStorage.setItem('illya_appearance', JSON.stringify({ locale, mode }));
  }, { locale, mode });
  await page.route('**/api/v1/me', (route) => route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } }));
}

const connectionLocales = [
  {
    locale: 'en', guide: 'Launcher operation guide',
    restart: /the launcher shows Restart; clicking it closes and relaunches the game\./,
    closeFirst: /(?:close (?:osu!|the game|your game).*first|first.*close|close.*before.*launch)/i,
    keyboard: false,
  },
  {
    locale: 'zh-TW', guide: '啟動器操作指南',
    restart: /啟動器會顯示重新啟動，由你按下後關閉遊戲並重新開啟。/,
    closeFirst: /(?:先關閉|關閉.*後再啟動|關閉.*再用啟動器)/,
    keyboard: true,
  },
  {
    locale: 'ja', guide: 'ランチャー操作ガイド',
    restart: /再起動が表示され、押すとゲームを閉じて起動し直します。/,
    closeFirst: /(?:先に.*閉じ|終了してから|閉じてから.*ランチャー)/,
    keyboard: true,
  },
];

for (const locale of connectionLocales) test(`Connect ${locale.locale} explains Restart and opens the launcher operation guide`, async ({ page }) => {
  await guestReader(page, locale.locale);
  await page.setViewportSize(locale.locale === 'ja' ? { width: 390, height: 844 } : { width: 1440, height: 1000 });
  const response = await page.goto('/connect');
  expect(response?.status()).toBe(200);
  await expect(page.locator('html')).toHaveClass(/dark/);
  const client = page.locator('#client');
  const stableCopy = client.getByRole('tabpanel').locator('p').first();
  await expect(stableCopy).toContainText(locale.restart);
  await expect(stableCopy).not.toContainText(locale.closeFirst);
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole('tab', { name: 'osu!lazer', exact: true }).click();
  const guide = client.getByRole('link', { name: locale.guide, exact: true });
  await expect(guide).toHaveAttribute('href', '/docs/kaleidoscope');
  await expect(page.locator('.guide-body a[href="/docs/lazer-client"]')).toHaveCount(0);
  if (locale.locale === 'en' && process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW === '1') {
    await page.evaluate(() => document.fonts.ready);
    await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
    await page.screenshot({
      path: '../../.impeccable/review/connection-docs-final-desktop-dark-connect.png', fullPage: false, animations: 'disabled',
    });
  }
  if (locale.keyboard) {
    await guide.focus();
    await expect(guide).toBeFocused();
    await page.keyboard.press('Enter');
  } else {
    await guide.click();
  }
  await expect(page).toHaveURL(/\/docs\/kaleidoscope$/);
  await expect(page.locator('.markdown-document').getByRole('heading', { level: 1, name: '使用 Kaleidoscope 啟動遊戲', exact: true })).toBeVisible();
});

test('the real connection guide follows Markdown filenames into public reader routes', async ({ page }) => {
  await guestReader(page, 'zh-TW', 'light');
  await page.setViewportSize({ width: 390, height: 844 });
  const response = await page.goto('/docs/connection');
  expect(response?.status()).toBe(200);
  await expect(page.locator('.markdown-document h1')).toBeVisible();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await expect.poll(() => page.locator('.markdown-document a').evaluateAll((links) => links.filter((link) => {
    const href = link.getAttribute('href') ?? '';
    return !/^(?:[a-z][a-z\d+.-]*:|\/\/)/i.test(href) && href.split(/[?#]/, 1)[0].endsWith('.md');
  }).length)).toBe(0);
  await expect(page.locator('.markdown-document a[href="https://github.com/ppy/osu-infrastructure/blob/master/score-submission.md"]')).toBeVisible();
  await page.locator('.markdown-document a[href="/docs/kaleidoscope"]').first().click();
  await expect(page).toHaveURL(/\/docs\/kaleidoscope$/);
  await expect(page.locator('.markdown-document h1')).toBeVisible();
  await page.locator('.markdown-document a[href="/docs/local-client"]').first().click();
  await expect(page).toHaveURL(/\/docs\/local-client$/);
  await expect(page.locator('.markdown-document h1')).toBeVisible();
});

const fragments = [
  { source: 'local-client', target: 'verification', id: 'native-map-discovery-checkpoint' },
  { source: 'protocol', target: 'realtime', id: 'reproducible-verification' },
  { source: 'protocol', target: 'native-chat', id: 'verification' },
  { source: 'protocol', target: 'verification', id: 'native-upload-and-lazer-discovery-checkpoint' },
  { source: 'verification', target: 'native-maps', id: 'lazer-官方圖譜集資訊' },
];

for (const fragment of fragments) test(`the authored ${fragment.id} fragment reaches its actual heading`, async ({ page }) => {
  const mobile = fragment.id === 'lazer-官方圖譜集資訊';
  await guestReader(page, mobile ? 'zh-TW' : 'en', mobile ? 'light' : 'dark');
  if (mobile) await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/docs/${fragment.source}`);
  const link = page.locator(`.markdown-document a[href="/docs/${fragment.target}#${encodeURIComponent(fragment.id)}"]`);
  await expect(link).toHaveCount(1);
  await link.click();
  await expect.poll(() => new URL(page.url()).pathname).toBe(`/docs/${fragment.target}`);
  await expect.poll(() => decodeURIComponent(new URL(page.url()).hash.slice(1))).toBe(fragment.id);
  const heading = page.locator(`.markdown-document [id="${fragment.id}"]`);
  await expect(heading).toBeVisible();
  await expect.poll(() => heading.evaluate((element) => {
    const header = document.querySelector('.site-header');
    return element.getBoundingClientRect().top - (header?.getBoundingClientRect().bottom ?? 0);
  })).toBeGreaterThanOrEqual(0);
  if (mobile && process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW === '1') {
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({
      path: '../../.impeccable/review/connection-docs-final-mobile-light-fragment.png', fullPage: false, animations: 'disabled',
    });
  }
  // Native same-document hash navigation targets the same rendered heading.
  await page.evaluate((id) => { location.hash = ''; location.hash = id; }, fragment.id);
  await expect.poll(() => decodeURIComponent(new URL(page.url()).hash.slice(1))).toBe(fragment.id);
  await expect(heading).toBeVisible();
  await page.reload();
  await expect(heading).toBeVisible();
  await expect.poll(() => heading.evaluate((element) => element.getBoundingClientRect().top)).toBeGreaterThanOrEqual(0);
});

test('the five added guides are served by the shipped reader', async ({ page }) => {
  await guestReader(page);
  for (const slug of ['appearance-state', 'profile-presentation', 'native-score-submission', 'mutation-authorization', 'website-chat']) {
    const response = await page.goto(`/docs/${slug}`);
    expect(response?.status()).toBe(200);
    await expect(page.locator('.markdown-document h1[id]')).toHaveCount(1);
  }
});

test('unknown and malicious docs routes return the not-found page', async ({ page }) => {
  await guestReader(page);
  for (const slug of ['unlisted-guide', '__proto__', 'constructor', 'toString', '%2e%2e%2fconnection', 'connection%5c..%5cpermissions']) {
    const response = await page.goto(`/docs/${slug}`);
    expect(response?.status(), slug).toBe(404);
    await expect(page.locator('.markdown-document')).toHaveCount(0);
  }
});

test('external sources stay exact while repository citations are readable without broken links', async ({ page }) => {
  await guestReader(page);
  await page.goto('/docs/architecture');
  await expect(page.locator('.markdown-document a[href="https://github.com/ppy/osu"]')).toBeVisible();
  await page.goto('/docs/protocol');
  await expect(page.locator('.markdown-document').getByText('social session parser tests', { exact: true })).toBeVisible();
  await expect(page.locator('.markdown-document').getByText('protocol smoke test', { exact: true })).toBeVisible();
  await expect(page.locator('.markdown-document').getByRole('link', { name: 'social session parser tests', exact: true })).toHaveCount(0);
  await expect(page.locator('.markdown-document').getByRole('link', { name: 'protocol smoke test', exact: true })).toHaveCount(0);
  await expect(page.locator('.markdown-document a[href^="../"]')).toHaveCount(0);
  await expect(page.locator('.markdown-document a[href="https://docs.rs/axum/latest/axum/extract/multipart/struct.MultipartError.html#method.status"]')).toBeVisible();
});
