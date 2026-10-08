import { test, expect, type Page as BrowserPage } from '@playwright/test';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import type { Beatmap, Leader, Preferences, Score, User } from '../src/lib/types';

// All community activity below is synthetic QA data intercepted in this browser.
// Production HomePage obtains its content solely from the existing public API.
const fixtureMaps: Beatmap[] = [
  {
    id: 6101,
    title: 'QA Fixture · Glass Reflection',
    artist: 'QA Artist',
    creator: 'QA Mapper',
    version: 'Insane',
    mode: 0,
    status: 'ranked',
    source: 'official',
    stars: 5.42,
    length_seconds: 123,
    official_id: 6101,
    bpm: 180,
    max_combo: 812,
    play_count: 500,
    md5: '1'.repeat(32),
    download_url: '/api/v1/beatmaps/6101/download',
    download_kind: 'osu',
    package: null,
  },
  {
    id: 6102,
    title: 'QA Fixture · Community Difficulty',
    artist: 'QA Artist',
    creator: 'QA Community Mapper',
    version: '4K Another',
    mode: 3,
    status: 'loved',
    source: 'local',
    stars: 4.24,
    length_seconds: 97,
    official_id: null,
    bpm: 150,
    max_combo: 1450,
    play_count: 101,
    md5: '2'.repeat(32),
    download_url: '/api/v1/beatmaps/6102/download',
    download_kind: 'osu',
    package: null,
  },
  {
    id: 6103,
    title: 'QA Fixture · Mapping Practice',
    artist: 'QA Artist',
    creator: 'QA Mapper',
    version: 'Taiko',
    mode: 1,
    status: 'pending',
    source: 'local',
    stars: 3.87,
    length_seconds: 134,
    official_id: null,
    bpm: 174,
    max_combo: 650,
    play_count: 9,
    md5: '3'.repeat(32),
    download_url: '/api/v1/beatmaps/6103/download',
    download_kind: 'osu',
    package: null,
  },
];
const fixtureLeaders: Leader[] = Array.from({ length: 5 }, (_, index) => ({
  id: 6201 + index,
  username: index === 0 ? 'QA Leading Player' : `QA Player ${index + 1}`,
  country: ['TW', 'JP', 'US', 'HK', 'DE'][index],
  avatar_url: null,
  pp: 12345.67 - index * 1010.12,
  rank: index + 1,
  play_count: 2400 - index * 300,
  accuracy: 99.5 - index * 0.1,
}));
const fixtureScores: Score[] = [
  {
    id: 6301,
    user_id: 6201,
    username: 'QA Leading Player',
    beatmap_id: 6101,
    title: fixtureMaps[0].title,
    version: 'Insane',
    mode: 0,
    score_variant: 'vanilla',
    score: 7654321,
    accuracy: 99.45,
    max_combo: 812,
    mods: 24,
    pp: 327.12,
    passed: true,
    verification: 'verified',
    created_at: '2026-10-07T12:30:00Z',
    has_replay: true,
  },
  {
    id: 6302,
    user_id: 6202,
    username: 'QA Player 2',
    beatmap_id: 6103,
    title: fixtureMaps[2].title,
    version: 'Taiko',
    mode: 1,
    score_variant: 'relax',
    score: 3456789,
    accuracy: 98.32,
    max_combo: 650,
    mods: 128,
    api_mods: [{ acronym: 'RX' }],
    pp: 204.87,
    passed: true,
    verification: 'verified',
    created_at: '2026-10-07T12:20:00Z',
    has_replay: false,
  },
  {
    id: 6303,
    user_id: 6203,
    username: 'QA Player 3',
    beatmap_id: 6102,
    title: fixtureMaps[1].title,
    version: '4K Another',
    mode: 3,
    score_variant: 'vanilla',
    score: 998745,
    accuracy: 99.87,
    max_combo: 1420,
    mods: 0,
    pp: 221.5,
    passed: true,
    verification: 'verified',
    created_at: '2026-10-07T12:10:00Z',
    has_replay: true,
  },
  // Exercise the landing's additional protection if a response ever includes private data.
  {
    id: 6399,
    user_id: 6299,
    username: 'QA Private Pending Player',
    beatmap_id: 6101,
    title: 'QA PRIVATE PENDING SCORE',
    version: 'Private',
    mode: 0,
    score_variant: 'vanilla',
    score: 1,
    accuracy: 10,
    max_combo: 1,
    mods: 0,
    pp: 0,
    passed: false,
    verification: 'pending',
    created_at: '2026-10-07T12:00:00Z',
    has_replay: true,
  },
];

function preferences(locale: string, theme: 'dark' | 'light'): Preferences {
  return {
    locale,
    mode: theme,
    accent_color: '#f28eb6',
    background_url: null,
    background_blur: 0,
    background_dim: 0.76,
    panel_blur: 8,
    panel_opacity: 0.55,
    border_radius: 14,
  };
}
type FixtureState = {
  user: User | null;
  empty: boolean;
  statusFailure: boolean;
  mapFailure: boolean;
  leaderFailure?: boolean;
  scoreFailure?: boolean;
  holdMaps?: Promise<void>;
  requests: Record<string, number>;
};

async function fixture(
  page: BrowserPage,
  baseURL: string | undefined,
  locale: 'zh-TW' | 'en' | 'ja',
  theme: 'dark' | 'light',
  changes: Partial<FixtureState> = {},
) {
  const pref = preferences(locale, theme);
  await page
    .context()
    .addCookies([{ name: 'illya_locale', value: locale, url: baseURL ?? 'http://localhost:3000' }]);
  await page.addInitScript((pref) => {
    localStorage.setItem('theme', pref.mode);
    localStorage.setItem('illya_appearance', JSON.stringify(pref));
  }, pref);
  const state: FixtureState = {
    user: null,
    empty: false,
    statusFailure: false,
    mapFailure: false,
    requests: {},
    ...changes,
  };
  await page.route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url());
    const endpoint = url.pathname.slice('/api/v1'.length);
    state.requests[endpoint] = (state.requests[endpoint] ?? 0) + 1;
    if (endpoint === '/me')
      return route.fulfill(
        state.user ? { json: state.user } : { status: 401, json: { error: { code: 'qa_guest' } } },
      );
    if (endpoint === '/me/preferences') return route.fulfill({ json: pref });
    if (endpoint === '/announcements')
      return route.fulfill({ json: { items: [], total: 0, page: 1, page_size: 3 } });
    if (endpoint === '/status')
      return state.statusFailure
        ? route.fulfill({ status: 503, json: { error: { code: 'qa_status_unavailable' } } })
        : route.fulfill({
            json: {
              players_online: 12,
              registered_players: 12840,
              ranked_scores: 304512,
              beatmap_count: 7890,
              compatibility: {},
            },
          });
    if (endpoint === '/beatmaps') {
      expect(url.searchParams.get('page_size')).toBe('3');
      if (state.holdMaps) await state.holdMaps;
      return state.mapFailure
        ? route.fulfill({ status: 503, json: { error: { code: 'qa_maps_unavailable' } } })
        : route.fulfill({
            json: {
              items: state.empty ? [] : fixtureMaps,
              total: state.empty ? 0 : 3,
              page: 1,
              page_size: 3,
            },
          });
    }
    if (endpoint === '/leaderboard') {
      expect(Object.fromEntries(url.searchParams)).toEqual({
        mode: '0',
        variant: 'vanilla',
        page_size: '5',
      });
      return state.leaderFailure
        ? route.fulfill({ status: 503, json: { error: { code: 'qa_leaders_unavailable' } } })
        : route.fulfill({
            json: {
              items: state.empty ? [] : fixtureLeaders,
              total: state.empty ? 0 : 5,
              page: 1,
              page_size: 5,
            },
          });
    }
    if (endpoint === '/scores') {
      expect(Object.fromEntries(url.searchParams)).toEqual({
        verification: 'verified',
        page_size: '4',
      });
      return state.scoreFailure
        ? route.fulfill({ status: 503, json: { error: { code: 'qa_scores_unavailable' } } })
        : route.fulfill({
            json: {
              items: state.empty ? [] : fixtureScores,
              total: state.empty ? 0 : 4,
              page: 1,
              page_size: 4,
            },
          });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'qa_unused_endpoint' } } });
  });
  return state;
}

async function stableLayout(page: BrowserPage) {
  await page.evaluate(() => document.fonts.ready);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
}

async function capture(page: BrowserPage, name: string, state: string) {
  await stableLayout(page);
  const hierarchy = process.env.ILLYA_MAP_DISCOVERY_CAPTURE === '1';
  if (!hierarchy && process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW !== '1') return;
  await page.evaluate(() => {
    document.documentElement.style.setProperty('scroll-behavior', 'auto', 'important');
    (document.activeElement as HTMLElement | null)?.blur();
    window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
  });
  await page.mouse.move(0, 0);
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  const output = path.resolve(
    '../../.impeccable/review',
    `${hierarchy ? 'map-hierarchy-home' : 'community-home'}-${name}`,
  );
  await page.screenshot({ path: `${output}.png`, fullPage: true, animations: 'disabled' });
  const sourceFiles = [
    'src/features/home.tsx',
    'src/features/home.css',
    'src/app/globals.css',
    'src/app/layout.tsx',
    'messages/zh-TW.json',
    'messages/en.json',
    'messages/ja.json',
  ];
  const sources = Object.fromEntries(
    sourceFiles.map((file) => [
      file,
      createHash('sha256').update(readFileSync(file)).digest('hex'),
    ]),
  );
  writeFileSync(
    `${output}.json`,
    JSON.stringify(
      {
        captured_at: new Date().toISOString(),
        route: '/',
        viewport: page.viewportSize(),
        visual_facts: await page.evaluate(() => ({
          locale: document.documentElement.lang,
          theme: document.documentElement.className,
          fonts: document.fonts.status,
          scrollY: window.scrollY,
          documentOverflow: document.documentElement.scrollWidth > innerWidth,
          panelBlur: getComputedStyle(document.documentElement).getPropertyValue('--panel-blur'),
          surface: getComputedStyle(document.documentElement).getPropertyValue('--surface'),
          raised: getComputedStyle(document.documentElement).getPropertyValue('--surface-raised'),
        })),
        state,
        data: 'Synthetic QA fixtures intercepted at existing public API routes; no native-client or live activity claims.',
        sources,
      },
      null,
      2,
    ) + '\n',
  );
}

for (const width of [1440, 1254]) {
  test(`Traditional Chinese dark landing at ${width}px uses public data and real destinations`, async ({
    page,
    baseURL,
  }) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await fixture(page, baseURL, 'zh-TW', 'dark');
    await page.setViewportSize({ width, height: 1000 });
    await page.goto('/');
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW');
    await expect(page.locator('html')).toHaveClass(/dark/);
    const intro = page.locator('.home-introduction');
    await expect(intro.getByRole('heading', { level: 1 })).toHaveText('Illyaverse');
    await expect(intro.getByRole('link', { name: '建立帳號', exact: true })).toHaveAttribute(
      'href',
      '/register',
    );
    await expect(intro.getByRole('link', { name: '連線教學', exact: true })).toHaveAttribute(
      'href',
      '/connect',
    );
    await expect(page.locator('.home-server-status dd')).toHaveText([
      '12',
      '12,840',
      '304,512',
      '7,890',
    ]);
    await expect(page.locator('.home-catalogue .map-row')).toHaveCount(3);
    await expect(page.locator('.home-leader-list > li')).toHaveCount(5);
    await expect(page.locator('.home-score-list > li')).toHaveCount(3);
    await expect(page.getByText('QA PRIVATE PENDING SCORE')).toHaveCount(0);
    await expect(page.locator('.home-score-list a[href="/scores/6301"]')).toBeVisible();
    await expect(
      page.locator('.home-score-list a[href="/players/6202?mode=1&variant=relax"]'),
    ).toBeVisible();
    await expect(page.locator('.home-score-list a[href="/beatmaps/6101"]')).toBeVisible();
    await expect(
      page.locator('.home-score-list a[href="/api/v1/scores/6301/replay"]'),
    ).toHaveAccessibleName(/QA Leading Player/);
    await expect(page.locator('.home-score-list a[href="/api/v1/scores/6399/replay"]')).toHaveCount(
      0,
    );
    await expect(page.locator('.home-onboarding ol > li')).toHaveCount(3);
    await expect(
      page.locator('.home-onboarding a[href="https://osu.ppy.sh/home/download"]'),
    ).toHaveAttribute('rel', 'noreferrer');
    await expect(page.locator('.home-tools-directory a[href="/admin"]')).toHaveCount(0);
    await expect(page.locator('.home-tools-directory a[href="/beatmaps/upload"]')).toHaveCount(0);
    await capture(
      page,
      `${width}-dark-zh-TW-populated`,
      'Guest; populated public maps/rankings/verified scores with rejected private fixture excluded.',
    );
    expect(errors).toEqual([]);
  });
}

test('English light mobile keeps empty collections separate from failed services and retries only the requested data', async ({
  page,
  baseURL,
}) => {
  let releaseMaps: () => void = () => {};
  const holdMaps = new Promise<void>((resolve) => {
    releaseMaps = resolve;
  });
  const state = await fixture(page, baseURL, 'en', 'light', {
    empty: true,
    statusFailure: true,
    mapFailure: true,
    holdMaps,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await expect(page.locator('html')).toHaveClass(/light/);
  await expect(page.locator('.home-catalogue').getByRole('status')).toBeVisible();
  releaseMaps();
  await expect(page.locator('.home-catalogue').getByRole('alert')).toBeVisible();
  await expect(page.locator('.home-server-status')).toContainText('Server status unavailable');
  await expect(page.locator('.home-server-status dd')).toHaveCount(0);
  await expect(page.locator('.home-recent-scores')).toContainText('No public scores yet');
  await expect(page.locator('.home-ranking-preview')).toContainText(
    'No players in the rankings yet',
  );
  await capture(
    page,
    '390-light-en-error-empty',
    'Guest; status/map failures and empty public score/ranking collections.',
  );
  const scoreRequests = state.requests['/scores'];
  const leaderRequests = state.requests['/leaderboard'];
  state.empty = false;
  state.mapFailure = false;
  state.holdMaps = undefined;
  await page
    .locator('.home-catalogue')
    .getByRole('button', { name: 'Try again', exact: true })
    .click();
  await expect(page.locator('.home-catalogue .map-row')).toHaveCount(3);
  await expect(page.locator('.home-catalogue').getByRole('alert')).toHaveCount(0);
  expect(state.requests['/scores']).toBe(scoreRequests);
  expect(state.requests['/leaderboard']).toBe(leaderRequests);
  state.statusFailure = false;
  await page.getByRole('button', { name: 'Reload server status', exact: true }).click();
  await expect(page.locator('.home-server-status')).toContainText('Server available');
  await expect(page.locator('.home-server-status dd')).toHaveText([
    '12',
    '12,840',
    '304,512',
    '7,890',
  ]);
  await stableLayout(page);
});

test('Japanese compact signed-in landing shows only authorized tools and preserves public activity', async ({
  page,
  baseURL,
}) => {
  const user: User = {
    id: 6401,
    username: 'QA Signed Player',
    country: 'JP',
    bio: '',
    avatar_url: null,
    banner_url: null,
    banner_position_x: 50,
    banner_position_y: 50,
    accent_color: '#f28eb6',
    roles: ['QA Upload Role'],
    permissions: ['beatmaps.upload'],
    joined_at: '2026-10-01T00:00:00Z',
    pp: 0,
    play_count: 0,
  };
  await fixture(page, baseURL, 'ja', 'dark', { user });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
  await expect(page.locator('.home-introduction a[href="/players/6401"]')).toBeVisible();
  await expect(page.locator('.home-tools-directory a[href="/beatmaps/upload"]')).toBeVisible();
  await expect(page.locator('.home-tools-directory a[href="/beatmaps/import"]')).toHaveCount(0);
  await expect(page.locator('.home-tools-directory a[href="/admin"]')).toHaveCount(0);
  await expect(page.locator('.home-tools-directory a[href="/settings?tab=profile"]')).toBeVisible();
  await expect(page.locator('.home-tools-directory a[href="/settings?tab=account"]')).toBeVisible();
  await expect(page.locator('.home-tools-directory a[href="/settings?tab=social"]')).toBeVisible();
  await expect(page.locator('.home-score-list > li')).toHaveCount(3);
  await expect(page.getByText('QA PRIVATE PENDING SCORE')).toHaveCount(0);
  await capture(
    page,
    '390-dark-ja-authorized',
    'Signed-in upload-only account; import/admin hidden; public verified activity.',
  );
  await page.setViewportSize({ width: 320, height: 844 });
  await stableLayout(page);
});

test('signed-in staff shortcuts and independent score/ranking recovery remain accessible', async ({
  page,
  baseURL,
}) => {
  const user: User = {
    id: 6402,
    username: 'QA Staff',
    country: 'TW',
    bio: '',
    avatar_url: null,
    banner_url: null,
    banner_position_x: 50,
    banner_position_y: 50,
    accent_color: '#f28eb6',
    roles: ['QA Staff'],
    permissions: ['admin.view', 'beatmaps.import'],
    joined_at: '2026-10-01T00:00:00Z',
    pp: 0,
    play_count: 0,
  };
  const state = await fixture(page, baseURL, 'en', 'dark', {
    user,
    leaderFailure: true,
    scoreFailure: true,
  });
  await page.goto('/');
  await expect(page.locator('.home-tools-directory a[href="/admin"]')).toBeVisible();
  await expect(page.locator('.home-tools-directory a[href="/beatmaps/import"]')).toBeVisible();
  await expect(page.locator('.home-tools-directory a[href="/beatmaps/upload"]')).toHaveCount(0);
  await expect(page.locator('.home-ranking-preview').getByRole('alert')).toBeVisible();
  await expect(page.locator('.home-recent-scores').getByRole('alert')).toBeVisible();
  const mapRequests = state.requests['/beatmaps'];
  state.leaderFailure = false;
  await page
    .locator('.home-ranking-preview')
    .getByRole('button', { name: 'Try again', exact: true })
    .click();
  await expect(page.locator('.home-leader-list > li')).toHaveCount(5);
  await expect(page.locator('.home-recent-scores').getByRole('alert')).toBeVisible();
  state.scoreFailure = false;
  await page
    .locator('.home-recent-scores')
    .getByRole('button', { name: 'Try again', exact: true })
    .click();
  await expect(page.locator('.home-score-list > li')).toHaveCount(3);
  expect(state.requests['/beatmaps']).toBe(mapRequests);
  await stableLayout(page);
});
