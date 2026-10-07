import { test, expect } from '@playwright/test';

test('native score preserves settings and explains pp eligibility on desktop and mobile', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  // This response exists only in the browser test; no community rows are created.
  await page.route('**/api/v1/scores/987654', (route) =>
    route.fulfill({
      json: {
        id: 987654,
        user_id: 9,
        username: 'Replay fixture',
        beatmap_id: 12,
        title: 'Native score fixture',
        version: 'Test',
        mode: 0,
        score: 1000000,
        accuracy: 100,
        max_combo: 100,
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
        hit_counts: { n300: 100, misses: 0 },
        user: { id: 9, username: 'Replay fixture', country: 'TW', avatar_url: null },
        beatmap: { id: 12, artist: 'Illyaverse', title: 'Native score fixture', version: 'Test' },
      },
    }),
  );
  await page.goto('/scores/987654');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Native score fixture');
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
  if (process.env.ILLYA_CAPTURE_SCORE_REVIEW === '1')
    await page.screenshot({
      path: '../../data-score-review/native-score-desktop.png',
      fullPage: true,
    });
  await page.setViewportSize({ width: 390, height: 844 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  await page.getByLabel('語言', { exact: true }).selectOption('ja');
  await expect(page.getByRole('heading', { name: 'Mod 設定' })).toBeVisible();
  await expect(page.getByText('DT · 再生速度', { exact: true })).toBeVisible();
  if (process.env.ILLYA_CAPTURE_SCORE_REVIEW === '1')
    await page.screenshot({
      path: '../../data-score-review/native-score-mobile-ja.png',
      fullPage: true,
    });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  expect(errors).toEqual([]);
});
