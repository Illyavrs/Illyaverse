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
    artist: 'Fixture artist',
    version: 'Expert fixture',
    beatmapset_id: index === 2 ? null : 24313,
    stars: index === 2 ? null : 6.42,
    grade: index === 2 ? null : 'S',
    beatmap_max_combo: 220,
    hit_counts: { n300: 193, n100: 4, n50: 2, nmiss: 1 },
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
    glass_level: 55,
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
    if (url.pathname === '/api/v1/beatmaps')
      return route.fulfill({ json: { items: [], total: 0, page: 1, page_size: 5 } });
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
async function capture(page: Page, name: string, documentTop = true) {
  await page.evaluate(() => document.fonts.ready);
  if (documentTop)
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
      fullPage: documentTop,
      animations: 'disabled',
    });
}

test('best displays supplied pp order, weights, original scores and replay links', async ({
  page,
}) => {
  await fixture(page, 'zh-TW');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`/players/${ownerId}`);
  const list = page.locator('.profile-best-score-list');
  await expect(list.locator('li')).toHaveCount(3);
  const first = list.locator('li').first();
  await expect(first).toContainText('490.00');
  await expect(first.locator('.score-weight')).toHaveText('100.00%');
  await expect(list.locator('li').nth(1).locator('.score-weight')).toHaveText('95.00%');
  await expect(list.locator('li').nth(1).locator('.score-weighted-pp')).toHaveText('456.00');
  await expect(first.locator('a.score-title')).toHaveAttribute('href', '/scores/989201');
  await expect(first.locator('.profile-score-grade')).toHaveText('S');
  await expect(first.locator('.profile-score-stars')).toHaveText('6.42');
  await expect(first.locator('.profile-score-combo')).toContainText('199/220x');
  await expect(first.locator('.profile-score-hits [data-judgement="nmiss"]')).toContainText('1');
  await expect(list.locator('li').nth(1).locator('.profile-score-grade')).toHaveCount(0);
  await expect(list.locator('li').nth(1).locator('.profile-score-stars')).toHaveCount(0);
  await expect(first).toContainText('SV2');
  await first.locator('[data-slot="dropdown-menu-trigger"]').click();
  const menu = page.locator('.profile-score-menu');
  await expect(menu).toContainText('100,000');
  await expect(menu.locator('a[href$="/replay"]')).toHaveAttribute(
    'href',
    '/api/v1/scores/989201/replay',
  );
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await capture(page, 'desktop-dark-zh-best');
});

test('mobile light best failure names recovery and retry restores the collection', async ({
  page,
}) => {
  const state = await fixture(page, 'en', 'light');
  state.failBest = true;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/players/${ownerId}`);
  const panel = page.locator('#profile-scores');
  await expect(panel.getByRole('alert')).toBeVisible();
  await expect(
    panel.getByRole('button', { name: 'Best performances', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
  await capture(page, 'mobile-light-en-error');
  state.failBest = false;
  await panel.getByRole('button', { name: 'Try again', exact: true }).click();
  await expect(page.locator('.profile-best-score-list li')).toHaveCount(3);
  await expect(panel.getByRole('alert')).toHaveCount(0);
  const resultLink = panel.locator('a.score-title').first();
  await resultLink.focus();
  await expect(resultLink).toBeFocused();
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
});

for (const locale of ['zh-TW', 'en', 'ja']) {
  for (const theme of ['dark', 'light']) {
    test(`compact score rows preserve hierarchy and keyboard actions in ${locale} ${theme}`, async ({
      page,
    }) => {
      const state = await fixture(page, locale, theme);
      state.bestCount = 12;
      await page.setViewportSize({ width: 1254, height: 714 });
      await page.goto(`/players/${ownerId}`);
      const list = page.locator('.profile-best-score-list');
      await expect(list.locator('li')).toHaveCount(12);
      const first = list.locator('li').first();
      await expect(first.locator('.profile-score-grade')).toHaveText('S');
      await expect(first.locator('.profile-score-details')).toContainText('99.00');
      await expect(first.locator('.profile-score-combo')).toContainText('199/220x');
      await page.evaluate(() => document.fonts.ready);
      const desktopRow = await first.boundingBox();
      const nextDesktopRow = await list.locator('li').nth(1).boundingBox();
      expect(desktopRow?.height).toBeLessThanOrEqual(50);
      expect(nextDesktopRow!.y - desktopRow!.y - desktopRow!.height).toBeLessThanOrEqual(4);
      const desktopActions = await first
        .locator('[data-slot="dropdown-menu-trigger"]')
        .boundingBox();
      expect(desktopActions?.width).toBeGreaterThanOrEqual(44);
      expect(desktopActions?.height).toBeGreaterThanOrEqual(44);
      await capture(page, `desktop-${theme}-${locale}-compact`);

      const actions = first.locator('[data-slot="dropdown-menu-trigger"]');
      await actions.focus();
      await page.keyboard.press('ArrowDown');
      const menu = page.locator('.profile-score-menu');
      await expect(menu).toBeVisible();
      await expect(menu.getByRole('menuitem').first()).toBeFocused();
      await page.keyboard.press('ArrowDown');
      await expect(menu.locator('a[href$="/replay"]')).toBeFocused();
      if (locale === 'en') await capture(page, `desktop-${theme}-${locale}-menu`, false);
      await page.keyboard.press('Escape');
      await expect(actions).toBeFocused();

      for (const width of [390, 320]) {
        await page.setViewportSize({ width, height: 844 });
        await expect(first.locator('.pp-value')).toBeVisible();
        await expect(first.locator('.profile-score-accuracy')).toBeVisible();
        await expect(first.locator('.profile-score-combo')).toBeVisible();
        await expect(actions).toBeVisible();
        const mobileRow = await first.boundingBox();
        expect(mobileRow?.height).toBeLessThanOrEqual(112);
        const mobileActions = await actions.boundingBox();
        expect(mobileActions?.width).toBeGreaterThanOrEqual(44);
        expect(mobileActions?.height).toBeGreaterThanOrEqual(44);
        const titles = await list.locator('.score-title strong').evaluateAll((elements) =>
          elements.map((element) => {
            const title = element as HTMLElement;
            const style = getComputedStyle(title);
            return {
              width: title.getBoundingClientRect().width,
              height: title.clientHeight,
              scrollWidth: title.scrollWidth,
              scrollHeight: title.scrollHeight,
              fontSize: parseFloat(style.fontSize),
              lineHeight: parseFloat(style.lineHeight),
            };
          }),
        );
        for (const title of titles) {
          expect(title.width).toBeGreaterThanOrEqual(140);
          expect(title.fontSize).toBeGreaterThanOrEqual(13);
          expect(title.height).toBeLessThanOrEqual(title.lineHeight * 2 + 1);
          // The full fixture title, including its distinguishing index, must fit.
          expect(title.scrollWidth).toBeLessThanOrEqual(title.width + 1);
          expect(title.scrollHeight).toBeLessThanOrEqual(title.height + 1);
        }
        const supportingType = await first.evaluate((row) =>
          [
            '.profile-score-meta',
            '.profile-score-mod',
            '.profile-score-details',
            '.profile-score-details small',
            '.profile-score-contribution',
          ].map((selector) => parseFloat(getComputedStyle(row.querySelector(selector)!).fontSize)),
        );
        expect(Math.min(...supportingType)).toBeGreaterThanOrEqual(11);
        const titleArea = await first.locator('.profile-score-main').boundingBox();
        const combo = await first.locator('.profile-score-combo').boundingBox();
        const performance = await first.locator('.profile-score-performance').boundingBox();
        expect(titleArea!.x + titleArea!.width).toBeLessThanOrEqual(mobileActions!.x);
        expect(combo!.x + combo!.width).toBeLessThanOrEqual(performance!.x);
        expect(performance!.x + performance!.width).toBeLessThanOrEqual(mobileActions!.x);
        await capture(page, `mobile-${width}-${theme}-${locale}-compact`);
      }
    });
  }
}

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
  await expect(page.locator('.profile-best-score-list')).toHaveCount(0);
  expect(new URL(page.url()).searchParams.get('score_view')).toBe('recent');
});

test('rapid collection and mode changes preserve URL selection and browser history', async ({
  page,
}) => {
  const state = await fixture(page);
  state.bestCount = 26;
  await page.goto(`/players/${ownerId}`);
  await expect(page.locator('.profile-best-score-list li')).toHaveCount(24);
  await page.getByRole('button', { name: 'Recent scores', exact: true }).click();
  await page.getByRole('button', { name: 'taiko', exact: true }).click();
  await expect.poll(() => new URL(page.url()).searchParams.get('score_view')).toBe('recent');
  await expect(page.locator('.score-table')).toContainText('Private pending fixture');
  await expect.poll(() => state.queries.at(-1)?.mode).toBe('1');
  await page.getByRole('button', { name: 'Relax (RX)', exact: true }).click();
  await expect.poll(() => state.queries.at(-1)?.variant).toBe('relax');
  await page.getByRole('button', { name: 'Best performances', exact: true }).click();
  await expect(page.locator('.profile-best-score-list li')).toHaveCount(24);
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page.locator('.profile-best-score-list li')).toHaveCount(2);
  expect(new URL(page.url()).searchParams.get('page')).toBe('2');
  await page.goBack();
  await expect(page.locator('.profile-best-score-list li')).toHaveCount(24);
  await page.getByRole('button', { name: 'mania', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Relax (RX)', exact: true })).toBeDisabled();
  await expect.poll(() => state.queries.at(-1)?.variant).toBe('vanilla');
  expect(new URL(page.url()).searchParams.get('page')).toBeNull();
});

test('page two retains global server position and decayed contribution', async ({ page }) => {
  const state = await fixture(page);
  state.bestCount = 26;
  await page.goto(`/players/${ownerId}?mode=0&variant=vanilla&page=2`);
  const first = page.locator('.profile-best-score-list li').first();
  await expect(first).toContainText('Best performance fixture 25');
  await expect(first.locator('.profile-score-position')).toHaveText('25');
  await expect(first.locator('.score-weight')).toHaveText('29.20%');
  await expect(first.locator('.score-weighted-pp')).toHaveText('73.00');
  await page.getByRole('button', { name: 'Recent scores', exact: true }).click();
  await expect(page.locator('.profile-score-list li')).toHaveCount(2);
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
  await expect(page.locator('.profile-score-list li')).toHaveCount(1);
  await expect(page.locator('.score-table')).not.toContainText('Private pending fixture');
});

test('an empty out-of-range best page returns to its current last page', async ({ page }) => {
  const state = await fixture(page);
  await page.goto(`/players/${ownerId}?mode=0&variant=vanilla&page=999999`);
  await expect(page.locator('.profile-best-score-list li')).toHaveCount(3);
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
  await expect(page.locator('.profile-best-score-list li')).toHaveCount(3);
  await page.getByRole('button', { name: 'Recent scores', exact: true }).click();
  const panel = page.locator('#profile-scores');
  await expect(panel.getByRole('alert')).toBeVisible();
  await expect(page.locator('.score-table')).toHaveCount(0);
  state.denyRecent = false;
  await panel.getByRole('button', { name: 'Try again', exact: true }).click();
  await expect(page.locator('.score-table')).toContainText('Private pending fixture');
});
