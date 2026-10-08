import { expect, test, type Page } from '@playwright/test';
import { writeFileSync } from 'node:fs';

// Intercepted synthetic content only. Runtime request gates verify real statistics and ownership.
const playerId = 989610;
const stamp = '2026-10-08T00:00:00Z';
const grades = { ssh: 2, ss: 3, sh: 4, s: 5, a: 6, b: 7, c: 8, d: 9, f: 10 };
async function fixture(page: Page, locale = 'en', theme = 'dark', banner = false) {
  await page.context().addCookies([
    {
      name: 'illya_locale',
      value: locale,
      url: process.env.ILLYA_WEB_URL ?? 'http://localhost:3000',
    },
  ]);
  const preferences = {
    mode: theme,
    locale,
    accent_color: '#f28eb6',
    background_url: null,
    background_blur: 0,
    background_dim: 0.76,
    panel_blur: 8,
    panel_opacity: 0.55,
    border_radius: 14,
  };
  const player = {
    id: playerId,
    username: 'Profile layout fixture',
    country: 'TW',
    bio: 'A profile introduction with real line breaks.\nThe score list and uploaded maps have their own sections.',
    avatar_url: null,
    banner_url: banner ? '/media/profile-layout-fixture.svg' : null,
    banner_position_x: 25,
    banner_position_y: 80,
    accent_color: '#8270b8',
    roles: ['player'],
    permissions: [],
    joined_at: stamp,
    pp: 1245.67,
    play_count: 164,
    statistics: {
      mode: 0,
      score_variant: 'vanilla',
      pp: 1245.67,
      rank: 12,
      country_rank: 3,
      accuracy: 98.76,
      play_count: 164,
      maximum_combo: 1254,
      total_hits: 75432,
      ranked_score: 52165432,
      total_score: 75000123,
      score_totals_complete: false,
      score_scale: 'classic_display',
      grade_counts: grades,
    } as Record<string, unknown> | undefined,
  };
  const score = {
    id: 989611,
    user_id: playerId,
    username: player.username,
    beatmap_id: 989612,
    title: 'Synthetic long beatmap title for responsive profile score rows',
    version: 'Extra difficulty with a long version name',
    mode: 0,
    score_variant: 'vanilla',
    score: 9543123,
    accuracy: 99.25,
    max_combo: 923,
    mods: 72,
    pp: 234.56,
    passed: true,
    verification: 'verified',
    created_at: stamp,
    has_replay: true,
    replay_complete: true,
    position: 1,
    pp_weight: 1,
    weighted_pp: 234.56,
  };
  const map = {
    id: 989612,
    title: 'Uploaded map fixture',
    artist: 'Fixture artist',
    creator: 'Original mapper',
    version: 'Extra',
    mode: 0,
    status: 'pending',
    source: 'local',
    stars: 5.12,
    length_seconds: 185,
    uploaded_by: playerId,
  };
  const state = { player, failUploads: false, uploads: [] as URL[] };
  await page.route('**/media/profile-layout-fixture.svg', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="600" height="900"><rect width="600" height="900" fill="#434343"/><rect y="300" width="600" height="300" fill="#777"/></svg>',
    }),
  );
  await page.route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === '/api/v1/me') return route.fulfill({ json: { ...player, preferences } });
    if (url.pathname === '/api/v1/me/preferences') return route.fulfill({ json: preferences });
    if (url.pathname === '/api/v1/me/relationships')
      return route.fulfill({ json: { friends: [], blocks: [] } });
    if (url.pathname === `/api/v1/users/${playerId}`) {
      const relax = url.searchParams.get('variant') === 'relax';
      return route.fulfill({
        json: {
          ...state.player,
          statistics: state.player.statistics && {
            ...state.player.statistics,
            ...(relax
              ? {
                  score_variant: 'relax',
                  ranked_score: null,
                  total_score: null,
                  score_scale: 'unavailable_for_relax',
                }
              : {}),
          },
        },
      });
    }
    if (url.pathname === `/api/v1/users/${playerId}/rank-history`) {
      const days = Number(url.searchParams.get('days') ?? '90');
      const start = new Date(stamp);
      start.setUTCDate(start.getUTCDate() - days + 1);
      return route.fulfill({
        json: {
          mode: Number(url.searchParams.get('mode') ?? '0'),
          variant: url.searchParams.get('variant') ?? 'vanilla',
          days,
          start_date: start.toISOString().slice(0, 10),
          end_date: stamp.slice(0, 10),
          time_zone: 'UTC',
          collection_interval_seconds: 1800,
          last_captured_at: null,
          points: [],
        },
      });
    }
    if (
      url.pathname === `/api/v1/users/${playerId}/scores/best` ||
      url.pathname === '/api/v1/scores'
    )
      return route.fulfill({
        json: {
          items: [
            {
              ...score,
              mode: Number(url.searchParams.get('mode') ?? '0'),
              score_variant: url.searchParams.get('variant') ?? 'vanilla',
            },
          ],
          total: 1,
          page: 1,
          page_size: 24,
        },
      });
    if (url.pathname === '/api/v1/beatmaps') {
      state.uploads.push(url);
      return state.failUploads
        ? route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } })
        : route.fulfill({ json: { items: [map], total: 17, page: 1, page_size: 5 } });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'fixture_missing' } } });
  });
  return state;
}

async function capture(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
  const hierarchy = process.env.ILLYA_MAP_DISCOVERY_CAPTURE === '1';
  if (!hierarchy && process.env.ILLYA_WEB_SMOKE_CAPTURE !== '1') return;
  await expect(page.locator('.rank-history-empty-title')).toBeVisible();
  await page.evaluate(() => {
    document.documentElement.style.setProperty('scroll-behavior', 'auto', 'important');
    (document.activeElement as HTMLElement | null)?.blur();
    window.scrollTo({ top: 0, behavior: 'instant' });
  });
  await page.mouse.move(0, 0);
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  const output = `../../.impeccable/review/${hierarchy ? 'map-hierarchy-profile' : 'community-profile'}-${name}`;
  await page.screenshot({
    path: `${output}.png`,
    fullPage: true,
    animations: 'disabled',
  });
  if (hierarchy)
    writeFileSync(
      `${output}.json`,
      JSON.stringify(
        {
          viewport: page.viewportSize(),
          route: new URL(page.url()).pathname + new URL(page.url()).search,
          data: 'Synthetic intercepted profile; no real player activity or account changes.',
          visual_facts: await page.evaluate(() => ({
            locale: document.documentElement.lang,
            theme: document.documentElement.className,
            fonts: document.fonts.status,
            scrollY: window.scrollY,
            documentOverflow: document.documentElement.scrollWidth > innerWidth,
            profileAccent: getComputedStyle(
              document.querySelector('.profile-page')!,
            ).getPropertyValue('--user-accent'),
            selectedFill: getComputedStyle(
              document.querySelector('.profile-page')!,
            ).getPropertyValue('--selection-fill'),
          })),
        },
        null,
        2,
      ),
    );
}

test('compact no-banner profile exposes actual ranks, detailed statistics, grades and uploader navigation', async ({
  page,
}) => {
  const state = await fixture(page, 'zh-TW');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`/players/${playerId}?mode=0&variant=vanilla`);
  await expect(page.locator('.profile-masthead')).toHaveClass(/no-banner/);
  await expect(page.locator('.profile-cover')).toHaveCount(0);
  const overview = await page.locator('.profile-masthead').boundingBox();
  expect(overview!.height).toBeLessThan(360);
  await expect(page.locator('.profile-stats')).toContainText('#12');
  await expect(page.locator('.profile-stats')).toContainText('TW 排名');
  await expect(page.locator('.profile-stats')).toContainText('#3');
  await expect(page.locator('.profile-detail-stats')).toContainText('1,254x');
  await expect(page.locator('.profile-detail-stats')).toContainText('75,432');
  await expect(page.locator('.profile-detail-stats')).toContainText('52,165,432');
  await expect(page.locator('.profile-totals-note')).toContainText('並不完整');
  await expect(page.locator('.profile-grade-strip dd')).toHaveText(
    Object.values(grades).map(String),
  );
  await expect(page.locator('#profile-about')).toContainText('The score list and uploaded maps');
  await expect(page.locator('#profile-uploads .map-row')).toHaveCount(1);
  await expect(page.locator('#profile-uploads')).toContainText('17');
  await expect(
    page.locator('#profile-uploads').getByRole('link', { name: '所有上傳圖譜' }),
  ).toHaveAttribute('href', `/beatmaps?uploaded_by=${playerId}&source=local`);
  await expect.poll(() => state.uploads.length).toBeGreaterThan(0);
  expect(state.uploads[0].searchParams.get('uploaded_by')).toBe(String(playerId));
  expect(state.uploads[0].searchParams.get('source')).toBe('local');
  expect(state.uploads[0].searchParams.get('page_size')).toBe('5');
  await capture(page, 'desktop-1440-dark-zh-no-banner');
});

test('mobile banner crop matches profile settings and score rows keep all values with keyboard links', async ({
  page,
}) => {
  await fixture(page, 'en', 'light', true);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/players/${playerId}`);
  await expect(page.locator('.profile-cover')).toHaveCSS('background-position', '25% 80%');
  const cover = await page.locator('.profile-cover').boundingBox();
  const avatar = await page.locator('.profile-identity > .avatar').boundingBox();
  expect(cover!.width / cover!.height).toBeCloseTo(12 / 5, 1);
  expect(cover!.y + cover!.height - avatar!.y).toBeCloseTo(avatar!.height / 2, 0);
  await expect(page.locator('html')).toHaveClass(/light/);
  const row = page.locator('.profile-score-row').first();
  await expect(row.locator('.profile-score-mod')).toHaveText(['HD', 'DT']);
  await expect(row).toContainText('99.25%');
  await expect(row).toContainText('923x');
  await expect(row.locator('.score-weight')).toHaveText('100.00%');
  await expect(row.locator('.score-weighted-pp')).toHaveText('234.56');
  await row.locator('a.score-title').focus();
  await expect(row.locator('a.score-title')).toBeFocused();
  const actions = row.locator('[data-slot="dropdown-menu-trigger"]');
  await actions.focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menu')).toContainText('9,543,123');
  const replay = page.getByRole('menuitem', { name: /^Download replay:/ });
  await page.keyboard.press('ArrowDown');
  await expect(replay).toBeFocused();
  await expect(replay).toHaveAttribute('href', '/api/v1/scores/989611/replay');
  await page.keyboard.press('Escape');
  await expect(actions).toBeFocused();
  await capture(page, 'mobile-light-en-banner');
});

test('Relax exposes unavailable totals honestly and preserves the selected mode through section jumps', async ({
  page,
}) => {
  await fixture(page);
  await page.goto(`/players/${playerId}?mode=1&variant=relax`);
  await expect(page.locator('.profile-totals-note')).toContainText('unavailable for Relax');
  await expect(page.locator('.profile-detail-stats')).not.toContainText('52,165,432');
  await page
    .getByRole('navigation', { name: 'Profile sections' })
    .getByRole('link', { name: 'Score collection' })
    .click();
  expect(new URL(page.url()).hash).toBe('#profile-scores');
  expect(new URL(page.url()).searchParams.get('mode')).toBe('1');
  expect(new URL(page.url()).searchParams.get('variant')).toBe('relax');
});

test('missing optional statistics stay unknown and an upload-list failure retries independently in Japanese', async ({
  page,
}) => {
  const state = await fixture(page, 'ja');
  state.player.statistics = undefined;
  state.failUploads = true;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/players/${playerId}?mode=1&variant=relax`);
  await expect(page.locator('.profile-detail-stats dd').first()).toHaveText('—');
  await expect(page.locator('.profile-grade-strip')).toHaveCount(0);
  await expect(page.locator('#profile-scores .profile-score-row')).toHaveCount(1);
  const uploads = page.locator('#profile-uploads');
  await expect(uploads.getByRole('alert')).toBeVisible();
  await capture(page, 'mobile-dark-ja-relax-error');
  state.failUploads = false;
  await uploads.getByRole('button', { name: '再試行', exact: true }).click();
  await expect(uploads.getByRole('alert')).toHaveCount(0);
  await expect(uploads.locator('.map-row')).toHaveCount(1);
});

for (const viewport of [
  { width: 1440, height: 1000 },
  { width: 1254, height: 714 },
]) {
  test(`populated Traditional Chinese desktop profile retains the chosen banner at ${viewport.width}px`, async ({
    page,
  }) => {
    await fixture(page, 'zh-TW', 'dark', true);
    await page.setViewportSize(viewport);
    await page.goto(`/players/${playerId}`);
    await expect(page.locator('.profile-masthead')).toHaveClass(/has-banner/);
    await expect(page.locator('.profile-grade-strip dd')).toHaveCount(9);
    await expect(page.locator('#profile-uploads .map-row')).toHaveCount(1);
    await expect(page.locator('.profile-cover')).toHaveCSS('background-position', '25% 80%');
    const cover = await page.locator('.profile-cover').boundingBox();
    const avatar = await page.locator('.profile-identity > .avatar').boundingBox();
    const masthead = await page.locator('.profile-masthead').boundingBox();
    const rail = await page.locator('.profile-section-nav').boundingBox();
    expect(cover!.y + cover!.height - avatar!.y).toBeCloseTo(avatar!.height / 2, 0);
    expect(rail!.y).toBeCloseTo(masthead!.y + masthead!.height, 0);
    await capture(page, `desktop-${viewport.width}-dark-zh-banner`);
  });
}

test('English mobile profile uses a compact header when no banner is uploaded', async ({
  page,
}) => {
  await fixture(page, 'en', 'light');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/players/${playerId}`);
  await expect(page.locator('.profile-masthead')).toHaveClass(/no-banner/);
  await expect(page.locator('.profile-cover')).toHaveCount(0);
  const overview = await page.locator('.profile-masthead').boundingBox();
  expect(overview!.height).toBeLessThan(420);
  await expect(page.locator('#profile-uploads .map-row')).toHaveCount(1);
  await expect(page.locator('#profile-scores .profile-score-row')).toHaveCount(1);
  const gradeBounds = await page.locator('#profile-grades-hint').evaluate((hint) => {
    const bounds = hint.getBoundingClientRect();
    const parent = hint.closest('.profile-grade-strip')!.getBoundingClientRect();
    return {
      left: bounds.left,
      right: bounds.right,
      parentLeft: parent.left,
      parentRight: parent.right,
    };
  });
  expect(gradeBounds.left).toBeGreaterThanOrEqual(gradeBounds.parentLeft);
  expect(gradeBounds.right).toBeLessThanOrEqual(gradeBounds.parentRight);
  await capture(page, 'mobile-light-en-no-banner');
});

test('placement gradients follow absolute-rank boundaries independently and never color an unranked value', async ({
  page,
}) => {
  const state = await fixture(page);
  const placement = page.locator('.profile-placement');
  const cases = [
    [1, 'first'],
    [2, 'top-ten'],
    [10, 'top-ten'],
    [11, 'top-hundred'],
    [100, 'top-hundred'],
    [101, 'top-thousand'],
    [1_000, 'top-thousand'],
    [1_001, 'top-ten-thousand'],
    [10_000, 'top-ten-thousand'],
    [10_001, 'ranked'],
  ] as const;
  for (const [index, [rank, tier]] of cases.entries()) {
    state.player.statistics!.rank = rank;
    state.player.statistics!.country_rank = 1;
    await page.goto(`/players/${playerId}?mode=${index % 4}&variant=vanilla`);
    await expect(placement.first()).toHaveAttribute('data-rank-tier', tier);
    await expect(placement.first().locator('dd')).toHaveText(`#${rank.toLocaleString('en')}`);
    await expect(placement.last()).toHaveAttribute('data-rank-tier', 'first');
    await expect(placement.first().locator('.profile-rank-number')).toHaveCSS(
      'background-image',
      /linear-gradient/,
    );
  }
  state.player.pp = 0;
  state.player.statistics!.rank = 1;
  await page.goto(`/players/${playerId}?mode=2`);
  await expect(placement.first()).toHaveAttribute('data-rank-tier', 'unranked');
  await expect(placement.last()).toHaveAttribute('data-rank-tier', 'unranked');
  await expect(placement.locator('.profile-rank-number')).toHaveCount(0);
  await expect(placement.locator('dd')).toHaveText(['—', '—']);
  state.player.pp = 10;
  for (const [index, rank] of [null, 0, -1, 1.5].entries()) {
    state.player.statistics!.rank = rank;
    await page.goto(`/players/${playerId}?mode=${index % 4}`);
    await expect(placement.first()).toHaveAttribute('data-rank-tier', 'unranked');
    await expect(placement.first().locator('dd')).toHaveText('—');
  }
});
