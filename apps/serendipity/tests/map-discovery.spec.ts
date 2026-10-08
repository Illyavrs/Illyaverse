import { test, expect, type Page, type Route } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

// All map/user data is synthetic. No account, mutation, map download or import is performed.
const stamp = '2026-10-08T00:00:00Z';
function difficulty(index: number, setIndex = 0) {
  const official = setIndex % 2 === 0;
  const id = 980000 + setIndex * 100 + index;
  return {
    id,
    native_set_id: official
      ? setIndex === 0
        ? 2588526
        : 1030378 + setIndex
      : 2_010_000_000 + setIndex,
    official_id: official ? id : null,
    title:
      setIndex === 0
        ? 'Discovery fixture song'
        : `Discovery fixture ${String(setIndex).padStart(2, '0')}`,
    artist: 'Synthetic catalogue artist',
    creator: 'Fixture mapper',
    version: `Difficulty ${index + 1}`,
    mode: index % 4,
    status: ['ranked', 'loved', 'pending', 'unranked'][index % 4],
    source: official ? 'official' : 'local',
    stars: Number((1.25 + index * 0.28).toFixed(2)),
    length_seconds: 91 + index,
    bpm: 180,
    max_combo: 400,
    play_count: 0,
    uploaded_by: 990501,
    md5: '0'.repeat(32),
    download_url: `/api/v1/beatmaps/${id}/download`,
    download_kind: 'osu',
    package: null,
    availability: { withdrawn: false, revision: 0, reason: null, changed_at: null },
  };
}
type FixtureDifficulty = ReturnType<typeof difficulty>;

async function fixture(page: Page, locale = 'en', theme = 'dark') {
  const appearance = {
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
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: locale, url: test.info().project.use.baseURL as string },
    ]);
  await page.addInitScript(
    (preferences) => localStorage.setItem('illya_appearance', JSON.stringify(preferences)),
    appearance,
  );
  const state = {
    maps: Array.from({ length: 30 }, (_, set) =>
      Array.from({ length: set === 0 ? 28 : 2 }, (_, index) => difficulty(index, set)),
    ).flat(),
    setQueries: [] as URLSearchParams[],
    selectorQueries: [] as string[],
    scoreQueries: [] as URLSearchParams[],
    populatedScores: false,
    failSets: false,
    failSelector: 0,
    holdSets: false,
    releaseSets: null as (() => void) | null,
    setReleases: [] as (() => void)[],
    holdSelector: false,
    releaseSelector: null as (() => void) | null,
    withdrawn: false,
    staff: false,
    coverRequests: [] as string[],
  };
  await page.route('https://assets.ppy.sh/**', async (route) => {
    state.coverRequests.push(route.request().url());
    // Optional review captures may render the real upstream cover in the browser.
    // No remote image is downloaded to the workspace or shipped as an asset.
    if (process.env.ILLYA_MAP_DISCOVERY_CAPTURE === '1') return route.continue();
    return route.abort();
  });
  const fail = (route: Route, status: number, code: string) =>
    route.fulfill({ status, json: { error: { code } } });
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const routePath = url.pathname.replace('/api/v1', '');
    const query = url.searchParams;
    if (request.method() !== 'GET')
      throw new Error(`Unexpected write in discovery fixture: ${request.method()} ${routePath}`);
    if (routePath === '/me')
      return state.staff
        ? route.fulfill({
            json: {
              id: 990501,
              username: 'Discovery fixture staff',
              country: 'TW',
              bio: '',
              avatar_url: null,
              banner_url: null,
              accent_color: appearance.accent_color,
              roles: [],
              permissions: ['admin.view', 'beatmaps.moderate'],
              joined_at: stamp,
              pp: 0,
              play_count: 0,
            },
          })
        : fail(route, 401, 'unauthorized');
    if (routePath === '/me/preferences') return route.fulfill({ json: appearance });
    if (routePath === '/beatmapsets') {
      state.setQueries.push(new URLSearchParams(query));
      if (state.holdSets)
        await new Promise<void>((resolve) => {
          state.releaseSets = resolve;
          state.setReleases.push(resolve);
        });
      if (state.failSets) return fail(route, 503, 'service_unavailable');
      const matched = state.maps.filter(
        (map) =>
          ['mode', 'status', 'source', 'uploaded_by'].every(
            (key) =>
              !query.has(key) || query.get(key) === String(map[key as keyof FixtureDifficulty]),
          ) &&
          (!query.get('q') ||
            `${map.title} ${map.artist} ${map.creator} ${map.version}`
              .toLowerCase()
              .includes(query.get('q')!.toLowerCase())),
      );
      const groups = [...new Set(matched.map((map) => map.native_set_id))].map((native_set_id) => {
        const matching = matched.filter((map) => map.native_set_id === native_set_id);
        return {
          native_set_id,
          representative: matching[0],
          matching_count: matching.length,
          stored_count: state.maps.filter((map) => map.native_set_id === native_set_id).length,
          matching_modes: [...new Set(matching.map((map) => map.mode))],
          matching_statuses: [...new Set(matching.map((map) => map.status))],
          stars_min: Math.min(...matching.map((map) => map.stars)),
          stars_max: Math.max(...matching.map((map) => map.stars)),
          length_min: Math.min(...matching.map((map) => map.length_seconds)),
          length_max: Math.max(...matching.map((map) => map.length_seconds)),
          added_at: stamp,
          preview: matching.slice(0, 8),
        };
      });
      if (query.get('sort') === 'title')
        groups.sort((a, b) => a.representative.title.localeCompare(b.representative.title));
      if (query.get('sort') === 'stars') groups.sort((a, b) => b.stars_max - a.stars_max);
      if (query.get('sort') === 'length') groups.sort((a, b) => b.length_max - a.length_max);
      const pageNumber = Number(query.get('page') ?? 1);
      return route.fulfill({
        json: {
          items: groups.slice((pageNumber - 1) * 24, pageNumber * 24),
          total: groups.length,
          page: pageNumber,
          page_size: 24,
        },
      });
    }
    const selector = /^\/beatmaps\/(\d+)\/set\/difficulties$/.exec(routePath);
    if (selector) {
      state.selectorQueries.push(`${routePath}${url.search}`);
      if (state.holdSelector)
        await new Promise<void>((resolve) => {
          state.releaseSelector = resolve;
        });
      if (state.failSelector)
        return fail(
          route,
          state.failSelector,
          state.failSelector === 410 ? 'beatmap_withdrawn' : 'service_unavailable',
        );
      if (state.withdrawn) return fail(route, 410, 'beatmap_withdrawn');
      const anchor = state.maps.find((map) => map.id === Number(selector[1]));
      const maps = state.maps.filter((map) => map.native_set_id === anchor?.native_set_id);
      const pageNumber = Number(query.get('page') ?? 1);
      return route.fulfill({
        json: {
          items: maps.slice((pageNumber - 1) * 24, pageNumber * 24),
          total: maps.length,
          page: pageNumber,
          page_size: 24,
        },
      });
    }
    const scores = /^\/beatmaps\/\d+\/scores$/.exec(routePath);
    if (scores) {
      state.scoreQueries.push(new URLSearchParams(query));
      const variant = query.get('variant') ?? 'vanilla';
      const mode = Number(query.get('mode') ?? 0);
      const items = state.populatedScores
        ? [
            {
              id: variant === 'relax' ? 971001 : 971002,
              user_id: 971101,
              username: variant === 'relax' ? 'RX fixture player' : 'Normal fixture player',
              beatmap_id: Number(routePath.split('/')[2]),
              title: 'Synthetic scoreboard fixture',
              version: 'Fixture difficulty',
              mode,
              score_variant: variant,
              position: 1,
              score: 800000,
              standardised_score: 800000,
              accuracy: 98.5,
              max_combo: 400,
              mods: variant === 'relax' ? 128 : 0,
              pp: 123.45,
              passed: true,
              verification: 'verified',
              created_at: stamp,
              has_replay: false,
            },
          ]
        : [];
      return route.fulfill({
        json: {
          items,
          total: items.length,
          page: Number(query.get('page') ?? 1),
          page_size: 24,
          mode,
          score_variant: variant,
          ordered_by: variant === 'relax' ? 'pp' : 'standardised_score',
        },
      });
    }
    const detail = /^\/(admin\/)?beatmaps\/(\d+)$/.exec(routePath);
    if (detail) {
      const map = state.maps.find((map) => map.id === Number(detail[2]));
      if (!map) return fail(route, 404, 'beatmap_not_found');
      if (state.withdrawn && !detail[1]) return fail(route, 410, 'beatmap_withdrawn');
      return route.fulfill({
        json: { ...map, availability: { ...map.availability, withdrawn: state.withdrawn } },
      });
    }
    return fail(route, 404, 'fixture_unhandled');
  });
  return state;
}

async function noOverflow(page: Page) {
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
}

async function capture(page: Page, name: string, locale: string, theme: string) {
  await page.evaluate(() => document.fonts.ready);
  await expect(page.locator('html')).toHaveClass(new RegExp(theme));
  await page.evaluate(() =>
    document.querySelectorAll<HTMLImageElement>('.map-artwork img').forEach((image) => {
      image.loading = 'eager';
    }),
  );
  await expect
    .poll(() =>
      page.evaluate(() =>
        [...document.querySelectorAll<HTMLImageElement>('.map-artwork img')].every(
          (image) => image.complete,
        ),
      ),
    )
    .toBe(true);
  await page.evaluate(() => window.scrollTo({ top: 0, left: 0, behavior: 'instant' }));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  await noOverflow(page);
  if (process.env.ILLYA_MAP_DISCOVERY_CAPTURE !== '1') return;
  const output = path.resolve(process.cwd(), '../../.impeccable/review');
  await mkdir(output, { recursive: true });
  const imagePath = path.join(output, `map-discovery-${name}.png`);
  await page.screenshot({ path: imagePath, fullPage: true, animations: 'disabled' });
  const metadata = await page.evaluate(() => ({
    width: innerWidth,
    height: innerHeight,
    scroll_x: scrollX,
    scroll_y: scrollY,
    document_width: document.documentElement.scrollWidth,
    document_height: document.documentElement.scrollHeight,
    actual_theme: document.documentElement.classList.contains('light') ? 'light' : 'dark',
    actual_locale: document.documentElement.lang,
    font_status: document.fonts.status,
  }));
  await writeFile(
    `${imagePath}.json`,
    JSON.stringify(
      {
        captured_at: new Date().toISOString(),
        url: page.url(),
        locale,
        theme,
        synthetic: true,
        artwork_origin:
          'https://assets.ppy.sh/beatmaps/{validated_official_set_id}/covers/cover.jpg; browser rendered only; fallback on failure',
        ...metadata,
      },
      null,
      2,
    ),
  );
}

test('set results show matched and stored counts, eight previews and server pages', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/beatmaps');
  await expect(page.locator('.discovery-set')).toHaveCount(24);
  await expect(page.locator('.discovery-result-count')).toHaveText(/^30 beatmap sets\s*Showing 1–24$/);
  await expect(page.locator('.discovery-result-count > span')).toHaveText('Showing 1–24');
  const first = page.locator('.discovery-set').first();
  await expect(first.getByText('28 matching · 28 stored difficulties')).toBeVisible();
  await expect(first.locator('.difficulty-chip')).toHaveCount(8);
  await expect(first.getByRole('link', { name: '20 more matching difficulties' })).toHaveAttribute(
    'href',
    '/beatmaps/980000?returnTo=%2Fbeatmaps#set-difficulties',
  );
  await expect(first.locator('.discovery-set-main .difficulty-chip')).toHaveCount(0);
  await expect(first.locator('.discovery-status')).toHaveCount(4);
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page).toHaveURL(/page=2/);
  await expect(page.locator('.discovery-set')).toHaveCount(6);
  expect(state.setQueries.at(-1)?.get('page')).toBe('2');
  await page.getByLabel('Game mode', { exact: true }).selectOption('3');
  await expect(page).not.toHaveURL(/page=2/);
  await expect(
    page.locator('.discovery-set').first().getByText('7 matching · 28 stored difficulties'),
  ).toBeVisible();
  await expect(page.locator('.discovery-set').first().locator('.discovery-status')).toHaveText([
    'Unranked',
  ]);
});

test('filter URLs restore through history; uploader clearing and layout keep other parameters', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto(
    '/beatmaps?q=fixture&mode=1&status=loved&source=official&sort=title&uploaded_by=990501&page=2&view=list&context=profile',
  );
  await expect(page.getByLabel('Search', { exact: true })).toHaveValue('fixture');
  await expect(page.getByLabel('Game mode', { exact: true })).toHaveValue('1');
  await expect(page.locator('.discovery-results')).toHaveClass(/view-list/);
  await expect(page.getByRole('button', { name: 'First page', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Clear uploader', exact: true }).click();
  await expect(page).not.toHaveURL(/uploaded_by|page=2/);
  const url = new URL(page.url());
  for (const [key, value] of Object.entries({
    q: 'fixture',
    mode: '1',
    status: 'loved',
    source: 'official',
    sort: 'title',
    view: 'list',
    context: 'profile',
  }))
    expect(url.searchParams.get(key)).toBe(value);
  await expect(page.locator('.discovery-set')).toHaveCount(15);
  const requests = state.setQueries.length;
  await page.getByRole('button', { name: 'Grid', exact: true }).click();
  await expect(page.locator('.discovery-results')).toHaveClass(/view-grid/);
  expect(state.setQueries).toHaveLength(requests);
  await page.getByLabel('Status', { exact: true }).selectOption('ranked');
  await expect(page.getByLabel('Status', { exact: true })).toHaveValue('ranked');
  await expect(page.locator('.discovery-set')).toHaveCount(0);
  await page.goBack();
  await expect(page.getByLabel('Status', { exact: true })).toHaveValue('loved');
  await expect(page.locator('.discovery-set')).toHaveCount(15);
  await page.goBack();
  await expect(page.locator('.discovery-results')).toHaveClass(/view-list/);
  await page.goForward();
  await expect(page.locator('.discovery-results')).toHaveClass(/view-grid/);
});

test('search is bounded in UTF-8 and pending, error, empty and invalid-page states discard results', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/beatmaps');
  await expect(page.locator('.discovery-set')).toHaveCount(24);
  state.holdSets = true;
  await page.getByLabel('Search', { exact: true }).fill('曲'.repeat(60));
  await expect(page.getByLabel('Search', { exact: true })).toHaveValue('曲'.repeat(33));
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await expect(page.locator('.discovery-set')).toHaveCount(0);
  await expect(page.locator('.discovery-skeleton')).toBeVisible();
  await expect.poll(() => state.setQueries.at(-1)?.get('q')).toBe('曲'.repeat(33));
  expect(
    new TextEncoder().encode(state.setQueries.at(-1)?.get('q') ?? '').length,
  ).toBeLessThanOrEqual(100);
  state.failSets = true;
  state.holdSets = false;
  state.releaseSets?.();
  await expect(page.getByRole('heading', { name: 'Beatmaps could not be loaded' })).toBeVisible();
  await expect(page.locator('.discovery-set')).toHaveCount(0);
  state.failSets = false;
  await page.getByRole('button', { name: 'Retry', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'No matching beatmap sets' })).toBeVisible();
  await page.getByRole('button', { name: 'Reset filters', exact: true }).first().click();
  await expect(page.locator('.discovery-set')).toHaveCount(24);
  await page.goto('/beatmaps?page=999');
  await expect(
    page.getByRole('heading', { name: 'This result page is no longer available' }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'First page', exact: true }).click();
  await expect(page.locator('.discovery-set')).toHaveCount(24);
});

test('known siblings work without an archive, page fully and preserve score variant selection', async ({
  page,
}) => {
  const state = await fixture(page);
  state.populatedScores = true;
  await page.goto('/beatmaps/980000?mode=0&variant=relax');
  const selector = page.locator('.set-difficulty-selector');
  await expect(selector.getByRole('heading', { name: 'Choose a difficulty' })).toBeVisible();
  await expect(selector.getByText('28 known difficulties in this set')).toBeVisible();
  await expect(selector.locator('.set-difficulty-row')).toHaveCount(24);
  await expect(selector.locator('.set-difficulty-strip .difficulty-chip')).toHaveCount(24);
  await expect(page.getByRole('link', { name: /Download.*\.osu/ })).toHaveCount(1);
  await expect(page.getByRole('link', { name: /Download.*\.osz/ })).toHaveCount(0);
  await expect.poll(() => state.scoreQueries.at(-1)?.get('variant')).toBe('relax');
  await expect(page.getByRole('link', { name: 'RX fixture player', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Normal', exact: true }).click();
  await expect.poll(() => state.scoreQueries.at(-1)?.get('variant')).toBe('vanilla');
  await expect(
    page.getByRole('link', { name: 'Normal fixture player', exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Relax (RX)', exact: true }).click();
  await expect(page).toHaveURL(/variant=relax/);
  await expect(page.getByRole('button', { name: 'Relax (RX)', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(page.getByRole('button', { name: 'Normal', exact: true })).toHaveAttribute(
    'aria-pressed',
    'false',
  );
  await expect(page.getByRole('link', { name: 'RX fixture player', exact: true })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Normal fixture player', exact: true })).toHaveCount(
    0,
  );
  await selector.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(selector.locator('.set-difficulty-row')).toHaveCount(4);
  await selector.locator('.set-difficulty-summary').click();
  expect(state.selectorQueries.at(-1)).toContain('page=2&page_size=24');
  await selector.locator('.set-difficulty-row').first().click();
  await expect(page).toHaveURL(/\/beatmaps\/980024/);
  await expect.poll(() => state.scoreQueries.at(-1)?.get('variant')).toBe('relax');
  await expect(selector.locator('.set-difficulty-row')).toHaveCount(24);
  await expect(selector.locator('.set-difficulty-disclosure')).not.toHaveAttribute('open', '');
  await expect(selector.locator('.set-difficulty-row[aria-current="page"]')).toHaveCount(0);
  await selector.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(selector.locator('.set-difficulty-row[aria-current="page"]')).toHaveCount(1);
});

test('difficulty details start collapsed, expand with a keyboard and keep the leaderboard nearby', async ({
  page,
}) => {
  await fixture(page);
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto('/beatmaps/980000');
  const selector = page.locator('.set-difficulty-selector');
  const disclosure = selector.locator('.set-difficulty-disclosure');
  const summary = selector.locator('.set-difficulty-summary');
  await expect(selector.locator('.set-difficulty-strip .difficulty-chip')).toHaveCount(24);
  await expect(disclosure).not.toHaveAttribute('open', '');
  await expect(selector.locator('.set-difficulty-row').first()).toBeHidden();
  await expect(summary).toHaveText('Difficulty details');
  await expect(selector.getByRole('button', { name: 'Next', exact: true })).toBeVisible();
  const leaderboard = page.getByRole('heading', { name: 'Beatmap leaderboard', exact: true });
  // A 24-row expanded list previously pushed this task beyond three screenfuls.
  expect(
    await leaderboard.evaluate((element) => element.getBoundingClientRect().top + window.scrollY),
  ).toBeLessThan(1440);
  await leaderboard.scrollIntoViewIfNeeded();
  await expect(leaderboard).toBeInViewport();
  await expect(disclosure).not.toHaveAttribute('open', '');
  await summary.focus();
  await summary.press('Enter');
  await expect(disclosure).toHaveAttribute('open', '');
  await expect(selector.locator('.set-difficulty-row[aria-current="page"]')).toBeVisible();
  await expect(selector.locator('.set-difficulty-row').first()).toContainText('Difficulty 1');
  await summary.press('Space');
  await expect(disclosure).not.toHaveAttribute('open', '');
  await expect(selector.locator('.set-difficulty-row').first()).toBeHidden();
  await summary.press('Enter');
  await selector.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(selector.locator('.set-difficulty-row')).toHaveCount(4);
  await expect(selector.locator('.set-difficulty-disclosure')).not.toHaveAttribute('open', '');
  await expect(selector.locator('.set-difficulty-strip .difficulty-chip')).toHaveCount(4);
});

test('returning to an earlier URL while requests are pending waits for fresh data', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/beatmaps');
  await expect(page.locator('.discovery-set')).toHaveCount(24);
  state.holdSets = true;
  await page.getByLabel('Game mode', { exact: true }).selectOption('1');
  await expect.poll(() => state.setQueries.at(-1)?.get('mode')).toBe('1');
  await expect(page.locator('.discovery-set')).toHaveCount(0);
  await page.goBack();
  await expect(page.getByLabel('Game mode', { exact: true })).toHaveValue('all');
  await expect.poll(() => state.setQueries.at(-1)?.get('mode')).toBe(null);
  await expect(page.locator('.discovery-set')).toHaveCount(0);
  await expect(page.locator('.discovery-skeleton')).toBeVisible();
  state.holdSets = false;
  state.failSets = true;
  state.setReleases.forEach((release) => release());
  await expect(page.getByRole('heading', { name: 'Beatmaps could not be loaded' })).toBeVisible();
  await expect(page.locator('.discovery-set')).toHaveCount(0);
});

test('difficulty navigation retains catalogue filters and RX; native mania forces Normal', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto(
    '/beatmaps?q=fixture&mode=0&status=ranked&source=official&sort=stars&uploaded_by=990501&view=list&context=profile',
  );
  await expect(page.locator('.discovery-set')).toHaveCount(15);
  const catalogue = new URL(page.url());
  const returnTo = `${catalogue.pathname}${catalogue.search}`;
  await page.locator('.discovery-set').first().locator('.difficulty-chip').first().click();
  await expect(page.locator('.back-link')).toHaveAttribute('href', returnTo);
  await page.getByRole('button', { name: 'Relax (RX)', exact: true }).click();
  const selector = page.locator('.set-difficulty-selector');
  await expect(selector.locator('.set-difficulty-row')).toHaveCount(24);
  await selector.locator('.set-difficulty-strip .difficulty-chip').nth(1).click();
  await expect(page).toHaveURL(/mode=1&variant=relax/);
  await expect.poll(() => state.scoreQueries.at(-1)?.get('variant')).toBe('relax');
  await expect.poll(() => state.scoreQueries.at(-1)?.get('mode')).toBe('1');
  await expect(page.locator('.back-link')).toHaveAttribute('href', returnTo);
  await expect(selector.locator('.set-difficulty-row')).toHaveCount(24);
  await selector.locator('.set-difficulty-strip .difficulty-chip').nth(3).click();
  await expect(page).toHaveURL(/mode=3&variant=vanilla/);
  await expect.poll(() => state.scoreQueries.at(-1)?.get('variant')).toBe('vanilla');
  await expect(page.getByRole('button', { name: 'Relax (RX)', exact: true })).toBeDisabled();
  await page.locator('.back-link').click();
  await expect(page).toHaveURL(`${catalogue.origin}${returnTo}`);
  await expect(page.getByLabel('Source', { exact: true })).toHaveValue('official');
  await expect(page.locator('.discovery-results')).toHaveClass(/view-list/);
  await expect(page.locator('.discovery-set')).toHaveCount(15);
});

test('catalogue back links reject external, protocol-relative, other-path and oversized returns', async ({
  page,
}) => {
  await fixture(page);
  for (const target of [
    'https://example.invalid/beatmaps',
    '//example.invalid/beatmaps',
    '/admin?tab=beatmaps',
    '/players/1',
    'javascript:alert(1)',
    `/beatmaps?q=${'x'.repeat(2049)}`,
  ]) {
    await page.goto(`/beatmaps/980000?${new URLSearchParams({ returnTo: target })}`);
    await expect(page.locator('.back-link')).toHaveAttribute('href', '/beatmaps');
  }
  await page.goto(
    `/beatmaps/980000?${new URLSearchParams({ returnTo: '/beatmaps?source=local&view=list' })}`,
  );
  await expect(page.locator('.back-link')).toHaveAttribute(
    'href',
    '/beatmaps?source=local&view=list',
  );
});

test('selector clears rows on page errors and withdrawn maps hide public known difficulties even for staff', async ({
  page,
}) => {
  const state = await fixture(page);
  state.staff = true;
  await page.goto('/beatmaps/980000');
  const selector = page.locator('.set-difficulty-selector');
  await expect(selector.locator('.set-difficulty-row')).toHaveCount(24);
  state.failSelector = 410;
  await selector.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(selector.locator('.set-difficulty-row')).toHaveCount(0);
  await expect(selector.locator('.difficulty-chip')).toHaveCount(0);
  await expect(
    selector.getByRole('heading', { name: 'This beatmap set is unavailable' }),
  ).toBeVisible();
  state.withdrawn = true;
  await page.goto('/beatmaps/980000?management=1');
  await expect(page.locator('.map-availability-summary')).toBeVisible();
  await expect(page.locator('.set-difficulty-selector')).toHaveCount(0);
  await expect(page.locator('.map-downloads')).toHaveCount(0);
  await page.goto('/beatmaps/980000');
  await expect(page.locator('.map-withdrawn-public')).toBeVisible();
  await expect(page.locator('.set-difficulty-selector')).toHaveCount(0);
});

test('official cover failures and local namespace fallbacks stay readable with long strings', async ({
  page,
}) => {
  const state = await fixture(page, 'en', 'light');
  state.maps[0].title = 'UnbrokenFixtureTitle'.repeat(20);
  state.maps[0].creator = 'LongMapperName'.repeat(20);
  state.maps[0].version = 'UnbrokenDifficultyName'.repeat(15);
  // An official source with an invalid/local native set ID must never request upstream art.
  state.maps.find((map) => map.native_set_id === 1030380)!.native_set_id = 2_100_000_000;
  await page.setViewportSize({ width: 320, height: 800 });
  await page.goto('/beatmaps');
  await expect(page.locator('.discovery-set')).toHaveCount(24);
  await expect(page.locator('.map-artwork-fallback').first()).toBeVisible();
  expect(state.coverRequests.every((url) => !url.includes('/2100000000/'))).toBe(true);
  await noOverflow(page);
  await page.getByRole('button', { name: 'List', exact: true }).click();
  await noOverflow(page);
  await page.locator('.discovery-set-main').first().click();
  await expect(page.locator('.set-difficulty-selector')).toBeVisible();
  await noOverflow(page);
});

test('review capture matrix uses document top and writes per-capture metadata', async ({
  page,
}) => {
  const state = await fixture(page, 'zh-TW', 'dark');
  state.maps = state.maps.filter((map) =>
    [2588526, 2_010_000_001, 2_010_000_003, 2_010_000_005].includes(map.native_set_id),
  );
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/beatmaps');
  await expect(page.locator('.discovery-set')).toHaveCount(4);
  await capture(page, 'desktop-dark-zh', 'zh-TW', 'dark');
  await page.setViewportSize({ width: 1280, height: 720 });
  await capture(page, '1280-dark-zh', 'zh-TW', 'dark');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole('button', { name: '列表', exact: true }).click();
  await capture(page, 'desktop-dark-zh-list', 'zh-TW', 'dark');
  await page.goto('/beatmaps/980000');
  await expect(page.locator('.set-difficulty-row')).toHaveCount(24);
  await capture(page, 'detail-dark-zh', 'zh-TW', 'dark');
  expect(state.selectorQueries).not.toHaveLength(0);
});

for (const { locale, theme, width, name } of [
  { locale: 'en', theme: 'light', width: 390, name: 'mobile-light-en' },
  { locale: 'ja', theme: 'dark', width: 320, name: 'mobile-dark-ja-320' },
])
  test(`review capture ${name}`, async ({ page }) => {
    const state = await fixture(page, locale, theme);
    state.maps = state.maps.filter((map) =>
      [2588526, 2_010_000_001, 2_010_000_003, 2_010_000_005].includes(map.native_set_id),
    );
    await page.setViewportSize({ width, height: 844 });
    await page.goto('/beatmaps');
    await expect(page.locator('.discovery-set')).toHaveCount(4);
    await capture(page, name, locale, theme);
  });
