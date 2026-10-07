import { test, expect, type Locator, type Page } from '@playwright/test';

// These responses exist only in the browser test; no community rows are created.
const nativeFixture = {
  id: 987654,
  user_id: 9,
  username: 'Replay fixture',
  beatmap_id: 12,
  title: 'Native score fixture',
  version: 'Test',
  mode: 0,
  score: 1000000,
  standardised_score: 1000000,
  standardised_without_mods: 1000000,
  classic_score: 100130,
  score_scale_status: 'native_reported',
  accuracy: 100,
  max_combo: 2,
  mods: 64,
  pp: 123.45,
  pp_eligible: false,
  api_mods: [
    { acronym: 'DT', settings: { speed_change: 1.2, adjust_pitch: false } },
    { acronym: 'DA', settings: { overall_difficulty: 9, approach_rate: 8.5 } },
  ],
  passed: true,
  verification: 'pending',
  created_at: '2026-10-07T00:00:00Z',
  has_replay: true,
  replay_complete: false,
  client: 'lazer',
  accuracy_source: 'client_reported',
  replay_url: '/api/v1/scores/987654/replay',
  hit_counts: { n300: 2, misses: 0 },
  user: { id: 9, username: 'Replay fixture', country: 'TW', avatar_url: null },
  beatmap: { id: 12, artist: 'Illyaverse', title: 'Native score fixture', version: 'Test' },
};

const conversionCopy = {
  'zh-TW': {
    raw: '原始分數',
    heading: '分數換算',
    standardised: '標準化分數',
    classic: 'Classic 顯示分數',
    native: 'lazer 提交的標準化分數；Classic 顯示換算使用伺服器從譜面計算的判定數。',
    estimated:
      '由 stable 成績摘要估算。無法重建每次判定的先後順序，因此不等同於精確的 lazer 遊玩分數。',
    unavailable: '這筆成績缺少可用的換算資訊，不會參與跨客戶端的分數比較。',
  },
  ja: {
    raw: '元のスコア',
    heading: 'スコア換算',
    standardised: '標準化スコア',
    classic: 'Classic 表示スコア',
    native:
      'lazer から送信されたスコアです。Classic 表示への換算には、サーバーが譜面から算出した判定数を使用します。',
    estimated:
      'stable の成績概要からの推定です。判定の順序は復元できないため、正確な lazer プレイのスコアとは異なります。',
    unavailable: '換算に必要な情報が不足しているため、クライアント間のスコア比較には含まれません。',
  },
};

function valueFor(container: Locator, label: string) {
  return container.locator('dt').filter({ hasText: label }).locator('..').locator('dd');
}

function conversionPanel(page: Page, heading: string) {
  return page.locator('section').filter({
    has: page.getByRole('heading', { name: heading, exact: true }),
  });
}

async function capture(page: Page, name: string) {
  if (process.env.ILLYA_CAPTURE_SCORE_REVIEW === '1')
    await page.screenshot({ path: `../../data-score-review/scale-${name}.png`, fullPage: true });
}

async function expectNoPageOverflow(page: Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
}

test('native score keeps raw and Classic values distinct with settings and eligibility', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.route('**/api/v1/scores/987654', (route) => route.fulfill({ json: nativeFixture }));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/scores/987654');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Native score fixture');

  const assertNativeScale = async (locale: keyof typeof conversionCopy) => {
    const copy = conversionCopy[locale];
    const panel = conversionPanel(page, copy.heading);
    await expect(valueFor(page.locator('.score-detail-values'), copy.raw)).toHaveText('1,000,000');
    await expect(valueFor(panel, copy.standardised)).toHaveText('1,000,000');
    await expect(valueFor(panel, copy.classic)).toHaveText('100,130');
    await expect(panel.getByText(copy.native, { exact: true })).toBeVisible();
    await expect(panel.getByText(copy.estimated, { exact: true })).toHaveCount(0);
    await expectNoPageOverflow(page);
  };
  await assertNativeScale('zh-TW');
  await expect(page.getByRole('heading', { name: 'Mod 參數' })).toBeVisible();
  await expect(page.locator('.score-detail-values')).toContainText('DT DA');
  await expect(page.getByText('DT · 播放速度', { exact: true })).toBeVisible();
  await expect(page.locator('dd').filter({ hasText: /^1\.2$/ })).toBeVisible();
  await expect(page.getByText('這組 Mod 不計入排名 pp，計算結果僅供參考。')).toBeVisible();
  await expect(page.getByText('Replay 擷取曾中斷，可能缺少部分影格。')).toBeVisible();
  await expect(page.getByRole('link', { name: 'Replay', exact: true })).toHaveAttribute(
    'href',
    '/api/v1/scores/987654/replay',
  );
  await capture(page, 'native-desktop-zh-TW');

  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByLabel('語言', { exact: true }).selectOption('ja');
  await assertNativeScale('ja');
  await expect(page.getByRole('heading', { name: 'Mod 設定' })).toBeVisible();
  await expect(page.getByText('DT · 再生速度', { exact: true })).toBeVisible();
  await capture(page, 'native-mobile-ja');
  expect(errors).toEqual([]);
});

test('legacy estimate preserves the raw result and explains the missing judgement order', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.route('**/api/v1/scores/987655', (route) =>
    route.fulfill({
      json: {
        ...nativeFixture,
        id: 987655,
        score: 2460,
        standardised_score: 303348,
        standardised_without_mods: 303348,
        classic_score: 31135,
        score_scale_status: 'legacy_estimated',
        client: 'stable',
        accuracy: 77.777778,
        max_combo: 5,
        mods: 0,
        api_mods: [],
        hit_counts: { n300: 7, misses: 2 },
        has_replay: false,
        replay_url: null,
      },
    }),
  );
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/scores/987655');
  const assertEstimate = async (locale: keyof typeof conversionCopy) => {
    const copy = conversionCopy[locale];
    const panel = conversionPanel(page, copy.heading);
    await expect(valueFor(page.locator('.score-detail-values'), copy.raw)).toHaveText('2,460');
    await expect(valueFor(panel, copy.standardised)).toHaveText('303,348');
    await expect(valueFor(panel, copy.classic)).toHaveText('31,135');
    await expect(panel.getByText(copy.estimated, { exact: true })).toBeVisible();
    await expect(panel.getByText(copy.native, { exact: true })).toHaveCount(0);
    await expectNoPageOverflow(page);
  };
  await assertEstimate('zh-TW');
  await capture(page, 'estimated-desktop-zh-TW');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByLabel('語言', { exact: true }).selectOption('ja');
  await assertEstimate('ja');
  await capture(page, 'estimated-mobile-ja');
  expect(errors).toEqual([]);
});

test('unavailable conversion excludes misleading comparison values in both locales', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.route('**/api/v1/scores/987656', (route) =>
    route.fulfill({
      json: {
        ...nativeFixture,
        id: 987656,
        score: 2460,
        standardised_score: null,
        standardised_without_mods: null,
        classic_score: null,
        score_scale_status: 'unavailable',
        client: 'stable',
        mods: 0,
        api_mods: [],
        has_replay: false,
        replay_url: null,
      },
    }),
  );
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/scores/987656');
  const assertUnavailable = async (locale: keyof typeof conversionCopy) => {
    const copy = conversionCopy[locale];
    const panel = conversionPanel(page, copy.heading);
    await expect(valueFor(page.locator('.score-detail-values'), copy.raw)).toHaveText('2,460');
    await expect(panel.getByText(copy.unavailable, { exact: true })).toBeVisible();
    await expect(panel.locator('dl')).toHaveCount(0);
    await expect(panel.getByText(copy.standardised, { exact: true })).toHaveCount(0);
    await expect(panel.getByText(copy.classic, { exact: true })).toHaveCount(0);
    await expectNoPageOverflow(page);
  };
  await assertUnavailable('zh-TW');
  await capture(page, 'unavailable-desktop-zh-TW');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByLabel('語言', { exact: true }).selectOption('ja');
  await assertUnavailable('ja');
  await capture(page, 'unavailable-mobile-ja');
  expect(errors).toEqual([]);
});
