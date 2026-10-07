import { test, expect, type Page } from '@playwright/test';

// Browser-only fixtures: no accounts, maps or scores are written to a community database.
const player = {
  id: 98431,
  username: 'RX browser fixture',
  country: 'TW',
  bio: '',
  roles: [],
  permissions: [],
  avatar_url: null,
  banner_url: null,
  joined_at: '2026-10-07T00:00:00Z',
};
const map = {
  id: 98432,
  official_id: null,
  title: 'RX browser map fixture',
  artist: 'Illyaverse',
  creator: 'Fixture',
  version: 'Normal',
  mode: 0,
  status: 'ranked',
  source: 'local',
  stars: 3,
  bpm: 180,
  length_seconds: 120,
  max_combo: 100,
  play_count: 0,
  md5: '0'.repeat(32),
};
function score(relax: boolean) {
  return {
    id: relax ? 98434 : 98433,
    user_id: player.id,
    username: player.username,
    beatmap_id: map.id,
    title: map.title,
    version: map.version,
    mode: 0,
    score_variant: relax ? 'relax' : 'vanilla',
    score: relax ? 0 : 1000000,
    standardised_score: relax ? null : 1000000,
    score_scale_status: relax ? 'unavailable' : 'native_reported',
    accuracy: 99,
    max_combo: 99,
    mods: relax ? 128 : 0,
    api_mods: relax ? [{ acronym: 'RX' }] : [],
    pp: relax ? 345 : 123,
    pp_eligible: true,
    passed: true,
    verification: 'verified',
    has_replay: true,
    created_at: '2026-10-07T00:00:00Z',
  };
}
const listing = (items: unknown[]) => ({ items, total: items.length, page: 1, page_size: 24 });
async function fixtures(page: Page) {
  await page.route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url());
    const relax = url.searchParams.get('variant') === 'relax';
    if (url.pathname === '/api/v1/me')
      return route.fulfill({ status: 401, json: { error: 'authentication_required' } });
    if (url.pathname === '/api/v1/leaderboard')
      return route.fulfill({
        json: listing([
          { ...player, pp: relax ? 345 : 123, play_count: relax ? 8 : 3, accuracy: 99, rank: 1 },
        ]),
      });
    if (url.pathname === `/api/v1/users/${player.id}`)
      return route.fulfill({
        json: {
          ...player,
          pp: relax ? 345 : 123,
          play_count: relax ? 8 : 3,
          statistics: {
            mode: Number(url.searchParams.get('mode')),
            score_variant: relax ? 'relax' : 'vanilla',
            pp: relax ? 345 : 123,
            rank: relax ? 2 : 1,
            country_rank: 1,
            play_count: relax ? 8 : 3,
            accuracy: 99,
          },
        },
      });
    if (url.pathname === `/api/v1/beatmaps/${map.id}`) return route.fulfill({ json: map });
    if (url.pathname === `/api/v1/users/${player.id}/scores/best`)
      return route.fulfill({
        json: {
          ...listing([
            { ...score(relax), position: 1, pp_weight: 1, weighted_pp: score(relax).pp },
          ]),
          collection: 'best',
          ordered_by: 'pp',
        },
      });
    if (url.pathname === '/api/v1/scores' || url.pathname === `/api/v1/beatmaps/${map.id}/scores`)
      return route.fulfill({ json: listing([score(relax)]) });
    if (url.pathname === '/api/v1/scores/98434')
      return route.fulfill({
        json: {
          ...score(true),
          user: player,
          beatmap: map,
          hit_counts: { n300: 99, misses: 1 },
          client: 'stable',
          accuracy_source: 'server_derived',
          replay_url: '/api/v1/scores/98434/replay',
        },
      });
    return route.continue();
  });
}
async function capturePair(page: Page, name: string) {
  if (process.env.ILLYA_CAPTURE_RELAX_REVIEW !== '1') return;
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.screenshot({
    path: `../../data-relax-review/${name}-desktop-zh-TW.png`,
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByLabel('語言', { exact: true }).selectOption('ja');
  await expect(page.getByLabel('言語', { exact: true })).toBeVisible();
  await page.screenshot({ path: `../../data-relax-review/${name}-mobile-ja.png`, fullPage: true });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
}

test('RX ranking switches isolated values and preserves URL context into a profile', async ({
  page,
}) => {
  await fixtures(page);
  await page.goto('/rankings');
  await expect(page.locator('.pp-value')).toHaveText('123 pp');
  await page.getByRole('button', { name: 'Relax (RX)', exact: true }).click();
  await expect(page).toHaveURL(/variant=relax/);
  await expect(page.locator('.pp-value')).toHaveText('345 pp');
  await expect(page.getByRole('link', { name: player.username })).toHaveAttribute(
    'href',
    /mode=0&variant=relax/,
  );
  await capturePair(page, 'rankings');
  await page.getByRole('link', { name: player.username }).click();
  await expect(page).toHaveURL(/players\/98431\?mode=0&variant=relax/);
  await expect(page.locator('.profile-stats')).toContainText('345');
});

test('profile uses mode and variant for statistics and score collection', async ({ page }) => {
  await fixtures(page);
  await page.goto(`/players/${player.id}?mode=1&variant=relax`);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(player.username);
  await expect(page.locator('.profile-stats')).toContainText('345');
  await expect(page.locator('.score-table')).toContainText('345.00');
  await expect(page.getByRole('button', { name: 'taiko', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await capturePair(page, 'profile');
  await page.getByRole('button', { name: 'mania', exact: true }).click();
  await expect(page).toHaveURL(/mode=3&variant=vanilla/);
  await expect(page.getByRole('button', { name: 'Relax (RX)', exact: true })).toBeDisabled();
  await expect(page.locator('.profile-stats')).toContainText('123');
});

test('map board requests best RX scores and links player context', async ({ page }) => {
  await fixtures(page);
  await page.goto(`/beatmaps/${map.id}?mode=0&variant=relax`);
  await expect(page.locator('.score-table')).toContainText('345.00');
  await expect(page.getByText('RX 依 pp 排序，每位玩家保留一筆最佳成績。')).toBeVisible();
  await expect(page.getByRole('link', { name: player.username })).toHaveAttribute(
    'href',
    /variant=relax/,
  );
  await capturePair(page, 'beatmap');
});

test('RX detail retains replay and partition links without inventing a converted score', async ({
  page,
}) => {
  await fixtures(page);
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/scores/98434');
  await expect(page.locator('.score-performance')).toContainText('Relax (RX)');
  await expect(page.getByRole('link', { name: 'Replay', exact: true })).toHaveAttribute(
    'href',
    '/api/v1/scores/98434/replay',
  );
  await expect(page.locator('.back-link')).toHaveAttribute('href', /mode=0&variant=relax/);
  await expect(
    page.getByText('這筆成績缺少可用的換算資訊，不會參與跨客戶端的分數比較。'),
  ).toBeVisible();
  await capturePair(page, 'score');
  expect(errors).toEqual([]);
});
