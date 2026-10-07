import { expect, test, type Page } from '@playwright/test';

// Synthetic intercepted requests only. PostgreSQL gates verify the actual ranking policy.
const ownerId = 989101;
const stamp = '2026-10-08T00:00:00Z';
function result(index: number, mode = 0, variant = 'vanilla') {
  const pp = 500 - index * 10;
  return {
    id: 989200 + index,
    user_id: ownerId,
    username: 'Profile best fixture',
    beatmap_id: 989300 + index,
    title: `Best performance fixture ${index}`,
    version: 'Expert fixture',
    mode,
    score_variant: variant,
    score: index === 1 ? 100000 : 9000000,
    standardised_score: variant === 'relax' ? null : 900000,
    score_scale_status: variant === 'relax' ? 'unavailable' : 'legacy_estimated',
    accuracy: 99,
    max_combo: 199,
    mods: variant === 'relax' ? 128 : 536870912,
    api_mods: null,
    pp,
    pp_eligible: true,
    passed: true,
    verification: 'verified',
    created_at: stamp,
    has_replay: index === 1,
    position: index,
    pp_weight: 0.95 ** (index - 1),
    weighted_pp: pp * 0.95 ** (index - 1),
  };
}
async function fixture(page: Page, locale = 'en', theme = 'dark') {
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
  const user = {
    id: ownerId,
    username: 'Profile best fixture',
    country: 'TW',
    bio: 'Synthetic profile QA.',
    avatar_url: null,
    banner_url: null,
    accent_color: '#f28eb6',
    roles: [],
    permissions: [],
    joined_at: stamp,
    pp: 946,
    play_count: 33,
    statistics: { rank: 1, accuracy: 99 },
    preferences,
  };
  const state = {
    viewer: true,
    bestCount: 3,
    failBest: false,
    denyRecent: false,
    queries: [] as { path: string; mode: string | null; variant: string | null; page: number }[],
  };
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname === '/api/v1/me')
      return route.fulfill(
        state.viewer
          ? { json: user }
          : { status: 401, json: { error: { code: 'authentication_required' } } },
      );
    if (url.pathname === '/api/v1/me/preferences') return route.fulfill({ json: preferences });
    if (url.pathname === '/api/v1/me/relationships')
      return route.fulfill({ json: { friends: [], blocks: [] } });
    if (url.pathname === '/api/v1/auth/logout') {
      state.viewer = false;
      return route.fulfill({ status: 204 });
    }
    if (url.pathname === `/api/v1/users/${ownerId}`)
      return route.fulfill({
        json: { ...user, pp: url.searchParams.get('variant') === 'relax' ? 1234 : 946 },
      });
    if (
      url.pathname === `/api/v1/users/${ownerId}/scores/best` ||
      url.pathname === '/api/v1/scores'
    ) {
      const best = url.pathname.endsWith('/best');
      const mode = Number(url.searchParams.get('mode') ?? '0');
      const variant = url.searchParams.get('variant') ?? 'vanilla';
      const pageNumber = Number(url.searchParams.get('page') ?? '1');
      state.queries.push({
        path: url.pathname,
        mode: url.searchParams.get('mode'),
        variant,
        page: pageNumber,
      });
      if ((best && state.failBest) || (!best && state.denyRecent))
        return route.fulfill({
          status: best ? 503 : 401,
          json: { error: { code: best ? 'unavailable' : 'authentication_required' } },
        });
      const publicRecent = { ...result(1, mode, variant), title: 'Public recent fixture' };
      const privateRecent = {
        ...result(2, mode, variant),
        title: 'Private pending fixture',
        verification: 'pending',
        has_replay: false,
      };
      const items = best
        ? Array.from({ length: state.bestCount }, (_, i) => result(i + 1, mode, variant))
        : state.viewer
          ? [privateRecent, publicRecent]
          : [publicRecent];
      return route.fulfill({
        json: {
          items: items.slice((pageNumber - 1) * 24, pageNumber * 24),
          total: items.length,
          page: pageNumber,
          page_size: 24,
          mode,
          score_variant: variant,
          ...(best ? { collection: 'best', ordered_by: 'pp' } : {}),
        },
      });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'fixture_missing' } } });
  });
  return state;
}
async function capture(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => {
    document.documentElement.style.setProperty('scroll-behavior', 'auto', 'important');
    (document.activeElement as HTMLElement | null)?.blur();
    window.scrollTo({ top: 0, behavior: 'instant' });
  });
  await page.mouse.move(0, 0);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
  if (process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW === '1')
    await page.screenshot({
      path: `../../.impeccable/review/profile-best-${name}.png`,
      fullPage: true,
      animations: 'disabled',
    });
}

test('best displays supplied pp order, weights, original scores and replay links', async ({
  page,
}) => {
  await fixture(page, 'zh-TW');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`/players/${ownerId}`);
  const table = page.locator('.profile-best-score-table');
  await expect(table.locator('tbody tr')).toHaveCount(3);
  const first = table.locator('tbody tr').first();
  await expect(first).toContainText('100,000');
  await expect(first).toContainText('490.00');
  await expect(first.locator('.score-weight')).toHaveText('100.00%');
  await expect(table.locator('tbody tr').nth(1).locator('.score-weight')).toHaveText('95.00%');
  await expect(table.locator('tbody tr').nth(1).locator('.score-weighted-pp')).toHaveText('456.00');
  await expect(first.locator('a.score-title')).toHaveAttribute('href', '/scores/989201');
  await expect(first.locator('a[href$="/replay"]')).toHaveAttribute(
    'href',
    '/api/v1/scores/989201/replay',
  );
  await expect(first).toContainText('SV2');
  await capture(page, 'desktop-dark-zh-best');
});

test('mobile light best failure names recovery and retry restores the collection', async ({
  page,
}) => {
  const state = await fixture(page, 'en', 'light');
  state.failBest = true;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/players/${ownerId}`);
  const panel = page.locator('.profile-layout section');
  await expect(panel.getByRole('alert')).toBeVisible();
  await expect(
    panel.getByRole('button', { name: 'Best performances', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
  await capture(page, 'mobile-light-en-error');
  state.failBest = false;
  await panel.getByRole('button', { name: 'Try again', exact: true }).click();
  await expect(page.locator('.profile-best-score-table tbody tr')).toHaveCount(3);
  await expect(panel.getByRole('alert')).toHaveCount(0);
  const region = panel.getByRole('region', { name: 'Best performances', exact: true });
  await region.focus();
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => region.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0);
});

test('Japanese empty best differs from owner recent pending history', async ({ page }) => {
  const state = await fixture(page, 'ja');
  state.bestCount = 0;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/players/${ownerId}`);
  await expect(
    page.getByText('対象のベストスコアはまだありません。', { exact: true }),
  ).toBeVisible();
  await capture(page, 'mobile-dark-ja-empty');
  await page.locator('.profile-score-collection button').nth(1).click();
  await expect(page.locator('.score-table')).toContainText('Private pending fixture');
  await expect(page.locator('.profile-best-score-table')).toHaveCount(0);
  expect(new URL(page.url()).searchParams.get('score_view')).toBe('recent');
});

test('rapid collection and mode changes preserve URL selection and browser history', async ({
  page,
}) => {
  const state = await fixture(page);
  state.bestCount = 26;
  await page.goto(`/players/${ownerId}`);
  await expect(page.locator('.profile-best-score-table tbody tr')).toHaveCount(24);
  await page.getByRole('button', { name: 'Recent scores', exact: true }).click();
  await page.getByRole('button', { name: 'taiko', exact: true }).click();
  await expect.poll(() => new URL(page.url()).searchParams.get('score_view')).toBe('recent');
  await expect(page.locator('.score-table')).toContainText('Private pending fixture');
  await expect.poll(() => state.queries.at(-1)?.mode).toBe('1');
  await page.getByRole('button', { name: 'Relax (RX)', exact: true }).click();
  await expect.poll(() => state.queries.at(-1)?.variant).toBe('relax');
  await page.getByRole('button', { name: 'Best performances', exact: true }).click();
  await expect(page.locator('.profile-best-score-table tbody tr')).toHaveCount(24);
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page.locator('.profile-best-score-table tbody tr')).toHaveCount(2);
  expect(new URL(page.url()).searchParams.get('page')).toBe('2');
  await page.goBack();
  await expect(page.locator('.profile-best-score-table tbody tr')).toHaveCount(24);
  await page.getByRole('button', { name: 'mania', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Relax (RX)', exact: true })).toBeDisabled();
  await expect.poll(() => state.queries.at(-1)?.variant).toBe('vanilla');
  expect(new URL(page.url()).searchParams.get('page')).toBeNull();
});

test('page two retains global server position and decayed contribution', async ({ page }) => {
  const state = await fixture(page);
  state.bestCount = 26;
  await page.goto(`/players/${ownerId}?mode=0&variant=vanilla&page=2`);
  const first = page.locator('.profile-best-score-table tbody tr').first();
  await expect(first).toContainText('Best performance fixture 25');
  await expect(first.locator('td').first()).toHaveText('25');
  await expect(first.locator('.score-weight')).toHaveText('29.20%');
  await expect(first.locator('.score-weighted-pp')).toHaveText('73.00');
  await page.getByRole('button', { name: 'Recent scores', exact: true }).click();
  await expect(page.locator('.score-table tbody tr')).toHaveCount(2);
  expect(new URL(page.url()).searchParams.get('page')).toBeNull();
});

test('logout removes private recent rows when returning to the same profile', async ({ page }) => {
  await fixture(page);
  const path = `/players/${ownerId}?score_view=recent`;
  await page.goto(path);
  await expect(page.locator('.score-table')).toContainText('Private pending fixture');
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page).toHaveURL(/\/$/);
  await page.goto(path);
  await expect(page.locator('.score-table tbody tr')).toHaveCount(1);
  await expect(page.locator('.score-table')).not.toContainText('Private pending fixture');
});

test('an empty out-of-range best page returns to its current last page', async ({ page }) => {
  const state = await fixture(page);
  await page.goto(`/players/${ownerId}?mode=0&variant=vanilla&page=999999`);
  await expect(page.locator('.profile-best-score-table tbody tr')).toHaveCount(3);
  await expect.poll(() => Number(new URL(page.url()).searchParams.get('page') ?? 1)).toBe(1);
  expect(state.queries.some((query) => query.page === 999999)).toBe(true);
  expect(state.queries.some((query) => query.page === 1)).toBe(true);
});

test('private recent access failure purges cached rows and recovers only after retry', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto(`/players/${ownerId}?score_view=recent`);
  await expect(page.locator('.score-table')).toContainText('Private pending fixture');
  state.denyRecent = true;
  await page.getByRole('button', { name: 'Best performances', exact: true }).click();
  await expect(page.locator('.profile-best-score-table tbody tr')).toHaveCount(3);
  await page.getByRole('button', { name: 'Recent scores', exact: true }).click();
  const panel = page.locator('.profile-layout section');
  await expect(panel.getByRole('alert')).toBeVisible();
  await expect(page.locator('.score-table')).toHaveCount(0);
  state.denyRecent = false;
  await panel.getByRole('button', { name: 'Try again', exact: true }).click();
  await expect(page.locator('.score-table')).toContainText('Private pending fixture');
});
