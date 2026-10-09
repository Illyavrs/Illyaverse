import { test, expect, type Page } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { selectField } from './helpers/field-select';

// Synthetic API metadata only: every API and cover request is intercepted.
function set(id = 42, count = 30) {
  return {
    official_set_id: id,
    title: id === 42 ? 'Public official fixture song' : `Public official fixture ${id}`,
    artist: 'Fixture artist 星の歌',
    creator: 'Fixture mapper',
    status: 'ranked',
    cover_url: `https://assets.ppy.sh/beatmaps/${id}/covers/card.jpg`,
    official_url: `https://osu.ppy.sh/beatmapsets/${id}`,
    download_disabled: false,
    difficulties: Array.from({ length: count }, (_, index) => ({
      official_id: id * 100 + index,
      version: `Difficulty ${index + 1}`,
      mode: index % 4,
      stars: Number((1.25 + index * 0.27).toFixed(2)),
      length_seconds: 90 + index,
      checksum: 'a'.repeat(32),
      bpm: 180,
      ar: 9,
      cs: 4,
      od: 8,
      hp: 6,
      max_combo: 500 + index,
      ...(index === 1 ? { local_id: 980001 } : {}),
    })),
    local: { stored_count: 1, archive_available: false, withdrawn: false },
  };
}

async function fixture(page: Page, locale = 'en', theme = 'dark') {
  const baseURL = test.info().project.use.baseURL as string;
  await page.context().addCookies([{ name: 'illya_locale', value: locale, url: baseURL }]);
  await page.addInitScript(
    ({ locale, theme }) =>
      localStorage.setItem(
        'illya_appearance',
        JSON.stringify({
          locale,
          mode: theme,
          accent_color: '#f28eb6',
          background_url: null,
          background_blur: 0,
          background_dim: 0.76,
          panel_blur: 8,
          panel_opacity: 0.55,
          border_radius: 14,
          glass_level: 100,
        }),
      ),
    { locale, theme },
  );
  const state = {
    searches: [] as URLSearchParams[],
    details: [] as number[],
    mutations: [] as string[],
    serverRequests: 0,
    failures: 0,
    empty: false,
    detailWithdrawn: false,
    detail: set(),
    hold: null as string | null,
    releases: [] as (() => void)[],
    cursor: 'opaque+/=&? page=next',
    settled: [] as string[],
  };
  await page.route('https://assets.ppy.sh/**', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="600" height="160"><rect width="600" height="160" fill="#38333d"/></svg>',
    }),
  );
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const routePath = url.pathname.replace('/api/v1', '');
    if (request.method() !== 'GET') {
      state.mutations.push(routePath);
      throw new Error(`Unexpected fixture mutation: ${routePath}`);
    }
    if (routePath === '/me')
      return route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } });
    if (routePath === '/official/beatmapsets/search') {
      state.searches.push(new URLSearchParams(url.searchParams));
      const q = url.searchParams.get('q') ?? '';
      if (q === state.hold) await new Promise<void>((resolve) => state.releases.push(resolve));
      state.settled.push(q);
      if (state.failures > 0) {
        state.failures--;
        return route.fulfill({
          status: 429,
          json: { error: { code: 'official_resolution_busy' } },
        });
      }
      const next = !!url.searchParams.get('cursor_string');
      const items = state.empty
        ? []
        : q === 'old'
          ? [{ ...set(44), title: 'STALE metadata must not appear' }]
          : [set(next ? 43 : 42), set(next ? 45 : 46, 2)];
      return route
        .fulfill({
          json: { items, total: state.empty ? 0 : 4, cursor_string: next ? null : state.cursor },
        })
        .catch(() => {});
    }
    if (/^\/official\/beatmapsets\/\d+$/.test(routePath)) {
      state.details.push(Number(routePath.split('/').at(-1)));
      if (state.detailWithdrawn)
        return route.fulfill({ status: 410, json: { error: { code: 'beatmapset_withdrawn' } } });
      return route.fulfill({ json: state.detail });
    }
    if (routePath === '/beatmapsets') {
      state.serverRequests++;
      return route.fulfill({ json: { items: [], page: 1, page_size: 24, total: 0 } });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'not_found' } } });
  });
  return state;
}

test('guest official catalogue uses direct search and full read-only difficulty previews', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/beatmaps');
  await expect(page.locator('.catalogue-source a').first()).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('.discovery-set')).toHaveCount(2);
  expect(state.serverRequests).toBe(0);
  expect(state.searches[0].get('status')).toBe('ranked');
  const initialSearches = state.searches.length;
  await page.locator('.discovery-difficulty-trigger').first().hover();
  const popup = page.locator('.discovery-difficulty-popover');
  await expect(popup.locator('.difficulty-preview-row')).toHaveCount(30);
  await expect(popup.locator('a')).toHaveCount(0);
  await expect(popup.getByText('Difficulty 30', { exact: true })).toBeAttached();
  expect(state.searches).toHaveLength(initialSearches);
  expect(state.details).toHaveLength(0);
  expect(state.mutations).toHaveLength(0);
  await expect(page.getByRole('link', { name: 'Import from osu!' })).toHaveCount(0);
});

test('filters, opaque cursor, layout and browser history keep the current official search', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/beatmaps');
  await expect(page.locator('.discovery-set')).toHaveCount(2);
  await page.getByRole('searchbox').fill('歌 & 星');
  await page
    .locator('.discovery-search')
    .getByRole('button', { name: 'Search', exact: true })
    .click();
  await expect.poll(() => state.searches.at(-1)?.get('q')).toBe('歌 & 星');
  await selectField(page.locator('#official-mode'), '3');
  await selectField(page.locator('#official-status'), 'loved');
  await selectField(page.locator('#official-sort'), 'difficulty_desc');
  await expect.poll(() => state.searches.at(-1)?.get('sort')).toBe('difficulty_desc');
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect.poll(() => state.searches.at(-1)?.get('cursor_string')).toBe(state.cursor);
  await expect(page.getByText('Public official fixture 43', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'List', exact: true }).click();
  await expect(page.locator('.discovery-results')).toHaveClass(/view-list/);
  expect(new URL(page.url()).searchParams.get('cursor')).toBe(state.cursor);
  await page.getByRole('button', { name: 'First page', exact: true }).click();
  await expect(page.getByText('Public official fixture song', { exact: true })).toBeVisible();
  await page.goBack();
  await expect(page.getByText('Public official fixture 43', { exact: true })).toBeVisible();
  expect(state.mutations).toHaveLength(0);
});

test('server catalogue is a simple separate source and retains existing uploader links', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/beatmaps');
  await expect(page.locator('.discovery-set')).toHaveCount(2);
  await page.locator('.catalogue-source').getByRole('link', { name: 'Server beatmaps' }).click();
  await expect(page).toHaveURL(/catalogue=server/);
  await expect.poll(() => state.serverRequests).toBe(1);
  await expect(page.getByRole('heading', { name: 'No beatmap sets yet' })).toBeVisible();
  await page.goto('/beatmaps?uploaded_by=99');
  await expect(page.locator('.catalogue-source a').last()).toHaveAttribute('aria-current', 'page');
  await expect.poll(() => state.serverRequests).toBe(2);
});

test('a held stale response never replaces newer results and requests drive the top loading bar', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/beatmaps');
  await expect(page.locator('.discovery-set')).toHaveCount(2);
  state.hold = 'old';
  await page.getByRole('searchbox').fill('old');
  await page
    .locator('.discovery-search')
    .getByRole('button', { name: 'Search', exact: true })
    .click();
  await expect.poll(() => state.releases.length).toBe(1);
  await expect(page.locator('.navigation-progress')).toHaveAttribute('data-pending', 'true');
  await page.getByRole('searchbox').fill('new');
  await page
    .locator('.discovery-search')
    .getByRole('button', { name: 'Search', exact: true })
    .click();
  await expect(page.getByText('Public official fixture song', { exact: true })).toBeVisible();
  state.releases.forEach((release) => release());
  await expect.poll(() => state.settled.includes('old')).toBe(true);
  await expect(page.getByText('STALE metadata must not appear', { exact: true })).toHaveCount(0);
  await expect(page.locator('.navigation-progress')).toHaveAttribute('data-pending', 'false');
});

test('busy retries retain filters and empty results have a working reset', async ({ page }) => {
  const state = await fixture(page);
  state.failures = 1;
  await page.goto('/beatmaps?q=remember&mode=2');
  await expect(page.locator('.discovery-feedback[role="alert"]')).toContainText(
    'Official search is busy',
  );
  await page.getByRole('button', { name: 'Retry', exact: true }).click();
  await expect(page.locator('.discovery-set')).toHaveCount(2);
  expect(state.searches.at(-1)?.get('q')).toBe('remember');
  state.empty = true;
  await page.getByRole('searchbox').fill('empty');
  await page
    .locator('.discovery-search')
    .getByRole('button', { name: 'Search', exact: true })
    .click();
  await expect(page.getByRole('heading', { name: 'No matching beatmap sets' })).toBeVisible();
  state.empty = false;
  await page.locator('.discovery-feedback').getByRole('button', { name: 'Reset filters' }).click();
  await expect(page.locator('.discovery-set')).toHaveCount(2);
  expect(state.searches.at(-1)?.get('q')).toBe('');
});

test('official detail presents every difficulty and only exact retained IDs link to server scores', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/beatmaps/official/42?returnTo=%2Fbeatmaps%3Fq%3Dremember');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Public official fixture song');
  await expect(page.locator('.public-official-difficulty-list button')).toHaveCount(30);
  await expect(
    page.getByText('This difficulty is not stored on this server yet.', { exact: true }),
  ).toBeVisible();
  await expect(page.getByRole('link', { name: 'Server leaderboard' })).toHaveCount(0);
  const initialDetails = state.details.length;
  await page.locator('.public-official-difficulty-list button').nth(1).click();
  await expect(page.getByRole('link', { name: 'Server leaderboard' })).toHaveAttribute(
    'href',
    '/beatmaps/980001?returnTo=%2Fbeatmaps%3Fq%3Dremember',
  );
  await expect(page.locator('.public-official-selected')).toContainText('501x');
  await expect(page.getByRole('link', { name: 'Download .osz' })).toHaveAttribute(
    'href',
    'https://osu.direct/api/d/42',
  );
  await expect(page.locator('.back-link')).toHaveAttribute('href', '/beatmaps?q=remember');
  expect([...new Set(state.details)]).toEqual([42]);
  expect(state.details).toHaveLength(initialDetails);
  expect(state.mutations).toHaveLength(0);
});

test('official disabled downloads and withdrawal cannot offer package or leaderboard access', async ({
  page,
}) => {
  const state = await fixture(page);
  state.detail.download_disabled = true;
  state.detail.local.archive_available = true;
  await page.goto('/beatmaps/official/42');
  await expect(page.getByRole('button', { name: 'Download .osz' })).toBeDisabled();
  await expect(
    page.getByText('osu! has disabled downloads for this set.', { exact: false }),
  ).toBeVisible();
  await expect(page.locator('a[href*="/archive"], a[href*="osu.direct"]')).toHaveCount(0);
  state.detailWithdrawn = true;
  await page.goto('/beatmaps/official/42?difficulty=4201');
  await expect(
    page.getByRole('heading', { name: 'This beatmap set is unavailable' }),
  ).toBeVisible();
  await expect(page.locator('.public-official-header, .public-official-difficulties')).toHaveCount(
    0,
  );
});

for (const variant of [
  { width: 1280, height: 720, locale: 'zh-TW', theme: 'dark' },
  { width: 1440, height: 900, locale: 'en', theme: 'light' },
  { width: 390, height: 844, locale: 'en', theme: 'light' },
  { width: 320, height: 740, locale: 'ja', theme: 'dark' },
]) {
  test(`public official layout ${variant.width}-${variant.theme}-${variant.locale}`, async ({
    page,
  }) => {
    await fixture(page, variant.locale, variant.theme);
    await page.setViewportSize({ width: variant.width, height: variant.height });
    const captureRoot = process.env.ILLYA_PUBLIC_MAP_CAPTURE_ROOT;
    async function capture(name: string) {
      if (!captureRoot) return;
      await page.evaluate(async () => {
        await document.fonts.ready;
      });
      await mkdir(captureRoot, { recursive: true });
      const nameBase = `public-official-${variant.width}-${variant.theme}-${variant.locale}-${name}`;
      const metrics = await page.evaluate(() => ({
        scrollY,
        width: innerWidth,
        height: innerHeight,
        documentWidth: document.documentElement.scrollWidth,
        bodyText: document.body.innerText,
        cards: Array.from(document.querySelectorAll('.discovery-set')).map((element) => ({
          height: element.getBoundingClientRect().height,
        })),
      }));
      await page.screenshot({ path: path.join(captureRoot, `${nameBase}.png`), fullPage: true });
      await writeFile(
        path.join(captureRoot, `${nameBase}.json`),
        JSON.stringify(
          {
            capturedAt: new Date().toISOString(),
            variant,
            syntheticApi: true,
            url: page.url(),
            metrics,
          },
          null,
          2,
        ),
      );
    }
    await page.goto('/beatmaps');
    await expect(page.locator('.discovery-set')).toHaveCount(2);
    await page.evaluate(async () => {
      await document.fonts.ready;
    });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    const heights = await page
      .locator('.discovery-set')
      .evaluateAll((elements) => elements.map((element) => element.getBoundingClientRect().height));
    expect(heights[0]).toBe(heights[1]);
    await capture('catalogue');
    await page.locator('.discovery-difficulty-trigger').first().click();
    await expect(page.locator('.discovery-difficulty-popover .difficulty-preview-row')).toHaveCount(
      30,
    );
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    await capture('preview');
    await page.goto('/beatmaps/official/42');
    await expect(page.locator('.public-official-difficulty-list button')).toHaveCount(30);
    await page.evaluate(async () => {
      await document.fonts.ready;
    });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    await capture('detail');
  });
}
