import { expect, test, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import english from '../messages/en.json';
import chinese from '../messages/zh-TW.json';
import japanese from '../messages/ja.json';
import { selectField } from './helpers/field-select';
import type { OfficialSearchResult, OfficialSearchSet } from '../src/features/official-map-search';
import type { Beatmap } from '../src/lib/types';

// Requests, accounts and archive bytes are synthetic and intercepted. These
// tests do not exercise official credentials, live storage or game clients.
const messages = { en: english, 'zh-TW': chinese, ja: japanese };
type Locale = keyof typeof messages;
const first: OfficialSearchSet = {
  official_set_id: 78120,
  title: 'Official search fixture with a long title and an extended movement name',
  artist: 'Search fixture artist with a long name',
  creator: 'Search fixture mapper',
  status: 'ranked',
  cover_url: 'https://assets.ppy.sh/beatmaps/78120/covers/list.jpg',
  official_url: 'https://osu.ppy.sh/beatmapsets/78120',
  download_disabled: false,
  difficulties: [
    { official_id: 91001, version: 'Normal', mode: 0, stars: 2.12, length_seconds: 137 },
    {
      official_id: 91002,
      version: 'A long extra difficulty name',
      mode: 0,
      stars: 5.37,
      length_seconds: 142,
    },
  ],
  local: { stored_count: 1, archive_available: false, withdrawn: false },
};
const second: OfficialSearchSet = {
  ...first,
  official_set_id: 78121,
  title: 'Second fixture',
  status: 'loved',
  official_url: 'https://osu.ppy.sh/beatmapsets/78121',
  difficulties: [{ ...first.difficulties[0], official_id: 91003, mode: 3 }],
  local: { stored_count: 1, archive_available: true, withdrawn: false },
};
const imported: Beatmap = {
  id: 91002,
  official_id: 91002,
  official_set_id: first.official_set_id,
  native_set_id: first.official_set_id,
  title: first.title,
  artist: first.artist,
  creator: first.creator,
  version: first.difficulties[1].version,
  mode: 0,
  status: 'ranked',
  source: 'official',
  stars: 5.37,
  length_seconds: 142,
  bpm: 173,
  max_combo: 604,
  play_count: 0,
  md5: 'a'.repeat(32),
  download_kind: 'osu',
  download_url: '/api/v1/beatmaps/91002/download',
  package: null,
};
const archiveFile = {
  name: 'search-fixture.osz',
  mimeType: 'application/octet-stream',
  buffer: Buffer.from('PK synthetic fixture'),
};
type Outcome = OfficialSearchResult | { status: number; code: string };
function response(
  items: OfficialSearchSet[] = [first, second],
  cursor: string | null = null,
): OfficialSearchResult {
  return { items, total: items.length, cursor_string: cursor };
}
function deferred() {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}
async function fixtures(
  page: Page,
  options: { locale?: Locale; permissions?: string[]; guest?: boolean; glass?: number } = {},
) {
  const locale = options.locale ?? 'en';
  const preferences = {
    mode: 'system',
    accent_color: '#f28eb6',
    background_url: null,
    background_blur: 0,
    background_dim: 0.76,
    panel_blur: 8,
    panel_opacity: 0.55,
    border_radius: 14,
    glass_level: options.glass ?? 100,
    locale,
  };
  const state = {
    queries: [] as URLSearchParams[],
    settledQueries: [] as string[],
    imports: [] as unknown[],
    archives: [] as string[],
    outcomes: [] as Outcome[],
    byQuery: new Map<string, OfficialSearchResult>(),
    holds: new Map<string, ReturnType<typeof deferred>>(),
    errors: [] as string[],
    unexpected: [] as string[],
  };
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: locale, url: test.info().project.use.baseURL as string },
    ]);
  await page.addInitScript(
    (value) => localStorage.setItem('illya_appearance', JSON.stringify(value)),
    preferences,
  );
  page.on('pageerror', (error) => state.errors.push(error.message));
  await page.route('https://assets.ppy.sh/**', (route) => route.abort());
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const pathname = url.pathname.replace('/api/v1', '');
    if (request.method() === 'GET' && pathname === '/me')
      return route.fulfill(
        options.guest
          ? {
              status: 401,
              json: { error: { code: 'unauthorized' } },
            }
          : {
              json: {
                id: 97200,
                username: 'Official search fixture',
                country: 'TW',
                bio: '',
                roles: [],
                permissions: options.permissions ?? ['beatmaps.import'],
                avatar_url: null,
                banner_url: null,
                accent_color: '#f28eb6',
                joined_at: '2026-10-09T00:00:00Z',
                pp: 0,
                play_count: 0,
              },
            },
      );
    if (request.method() === 'GET' && pathname === '/me/preferences')
      return route.fulfill({ json: preferences });
    if (request.method() === 'GET' && pathname === '/beatmaps/official/search') {
      state.queries.push(url.searchParams);
      const query = url.searchParams.get('q') ?? '';
      const outcome = state.byQuery.get(query) ?? state.outcomes.shift() ?? response();
      await state.holds.get(query)?.promise;
      // A held response may be aborted by a newer search or page teardown.
      return route
        .fulfill(
          'status' in outcome
            ? {
                status: outcome.status,
                json: { error: { code: outcome.code, message: 'RAW_UPSTREAM_ERROR' } },
              }
            : { json: outcome },
        )
        .catch(() => undefined)
        .finally(() => state.settledQueries.push(query));
    }
    if (request.method() === 'POST' && pathname === '/beatmaps/import') {
      state.imports.push(request.postDataJSON());
      return route.fulfill({ json: imported });
    }
    if (request.method() === 'POST' && pathname === '/beatmaps/import/archive') {
      state.archives.push(request.postDataBuffer()?.toString() ?? '');
      return route.fulfill({
        json: {
          items: [imported],
          package: {
            id: '04236a39-50aa-47b1-8533-af38e11b0a81',
            source: 'official',
            native_set_id: first.official_set_id,
            kind: 'osz',
            legacy: false,
            difficulty_count: 1,
            beatmap_ids: [imported.id],
            download_url: '/api/v1/beatmaps/91002/archive',
          },
          idempotent: false,
        },
      });
    }
    state.unexpected.push(`${request.method()} ${pathname}`);
    return route.fulfill({ status: 501, json: { error: { code: 'unexpected_fixture_request' } } });
  });
  return Object.assign(state, {
    copy: messages[locale],
    search: page.locator('.official-map-search'),
  });
}
function assertClean(state: Awaited<ReturnType<typeof fixtures>>) {
  expect(state.unexpected).toEqual([]);
  expect(state.errors).toEqual([]);
}
async function runSearch(page: Page, state: Awaited<ReturnType<typeof fixtures>>, query?: string) {
  if (query !== undefined) await page.locator('#official-search-query').fill(query);
  await state.search.getByRole('button', { name: state.copy.common.search, exact: true }).click();
}
async function capture(page: Page, name: string) {
  if (process.env.ILLYA_CAPTURE_OFFICIAL_SEARCH !== '1') return;
  const directory = path.resolve(__dirname, '../../../.impeccable/review');
  mkdirSync(directory, { recursive: true });
  await page.evaluate(async () => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    window.scrollTo({ top: 0, behavior: 'instant' });
    await document.fonts.ready;
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
    );
  });
  const metrics = await page.evaluate(() => ({
    width: innerWidth,
    document_width: document.documentElement.scrollWidth,
    theme: document.documentElement.className,
    glass: document.documentElement.dataset.glassLevel,
    material: getComputedStyle(document.querySelector('.official-map-search')!).backgroundColor,
  }));
  expect(metrics.document_width).toBeLessThanOrEqual(metrics.width);
  await page.screenshot({
    path: path.join(directory, `official-search-${name}.png`),
    fullPage: true,
    animations: 'disabled',
  });
  writeFileSync(
    path.join(directory, `official-search-${name}.json`),
    JSON.stringify(
      {
        provenance:
          'Synthetic intercepted official search/account/upload fixtures; no retained writes or official credentials.',
        ...metrics,
      },
      null,
      2,
    ),
  );
}

test('official search is explicit and prefills either flow while preserving inactive drafts and files', async ({
  page,
}) => {
  const state = await fixtures(page);
  await page.goto('/beatmaps/import');
  await expect(state.search).toBeVisible();
  await selectField(page.locator('#official-import-kind'), 'archive');
  await page.locator('#official_set_id').fill('99700');
  await page.locator('#official_archive_file').setInputFiles(archiveFile);
  await selectField(page.locator('#official-import-kind'), 'difficulty');
  await page.locator('#official_id').fill('99600');
  await page.locator('#official-search-query').fill('Fixture');
  await selectField(page.locator('#official-search-mode'), '0');
  await selectField(page.locator('#official-search-status'), 'loved');
  await selectField(page.locator('#official-search-sort'), 'date_desc');
  expect(state.queries).toHaveLength(0);
  await runSearch(page, state);
  await expect(state.search.locator('.official-search-set')).toHaveCount(2);
  expect(Object.fromEntries(state.queries[0])).toEqual({
    q: 'Fixture',
    mode: '0',
    status: 'loved',
    sort: 'date_desc',
  });
  await state.search.locator('.official-search-summary').first().click();
  await expect(state.search.locator('.official-search-summary').first()).toHaveAttribute(
    'aria-expanded',
    'true',
  );
  await selectField(page.locator(`#official-search-difficulty-${first.official_set_id}`), '91002');
  await state.search
    .getByRole('button', { name: state.copy.maps.officialSearch.useDifficulty, exact: true })
    .click();
  await expect(page.locator('#official_id')).toHaveValue('91002');
  await expect(page.locator('#official_id')).toBeFocused();
  expect(state.imports).toEqual([]);
  expect(state.archives).toEqual([]);
  await page
    .getByRole('button', { name: state.copy.maps.importDifficultyAction, exact: true })
    .click();
  await expect(page.locator('.map-upload-result').getByRole('status')).toHaveText(
    state.copy.maps.importSuccess,
  );
  expect(state.imports).toEqual([{ official_id: 91002 }]);
  await page.locator('#official_id').fill('99500');
  await state.search
    .getByRole('button', { name: state.copy.maps.officialSearch.useSet, exact: true })
    .click();
  await expect(page.locator('#official-import-kind')).toHaveAttribute('data-value', 'archive');
  await expect(page.locator('#official_set_id')).toHaveValue(String(first.official_set_id));
  await expect(page.locator('#official_archive_file')).toBeFocused();
  expect(
    await page
      .locator('#official_archive_file')
      .evaluate((input: HTMLInputElement) => input.files?.[0]?.name),
  ).toBe(archiveFile.name);
  await expect(page.locator('.map-upload-result')).toHaveCount(0);
  expect(state.archives).toHaveLength(0);
  await page
    .getByRole('button', { name: state.copy.maps.importArchiveAction, exact: true })
    .click();
  await expect(page.locator('.map-upload-result').getByRole('status')).toHaveText(
    state.copy.maps.importArchiveSuccess,
  );
  expect(state.archives[0]).toContain(`name="official_set_id"\r\n\r\n${first.official_set_id}`);
  expect(state.archives[0]).toContain(`filename="${archiveFile.name}"`);
  await selectField(page.locator('#official-import-kind'), 'difficulty');
  await expect(page.locator('#official_id')).toHaveValue('99500');
  assertClean(state);
});

test('empty query keeps official defaults and opaque paging deduplicates sets and stops repeated cursors', async ({
  page,
}) => {
  const state = await fixtures(page);
  const cursor = 'opaque +/=?:&% value';
  state.outcomes.push(
    { ...response([first], cursor), total: 3 },
    {
      ...response(
        [{ ...first, local: { ...first.local, archive_available: true } }, second],
        cursor,
      ),
      total: 3,
    },
  );
  await page.goto('/beatmaps/import');
  await runSearch(page, state);
  await expect(state.search.locator('.official-search-set')).toHaveCount(1);
  expect(Object.fromEntries(state.queries[0])).toEqual({
    q: '',
    mode: 'all',
    status: 'ranked',
    sort: 'ranked_desc',
  });
  await state.search
    .getByRole('button', { name: state.copy.maps.officialSearch.loadMore, exact: true })
    .click();
  await expect(state.search.locator('.official-search-set')).toHaveCount(2);
  expect(state.queries[1].get('cursor_string')).toBe(cursor);
  await expect(
    state.search.getByRole('button', {
      name: state.copy.maps.officialSearch.loadMore,
      exact: true,
    }),
  ).toHaveCount(0);
  await expect(state.search.locator('.official-search-set').first()).toContainText(
    state.copy.maps.officialSearch.archiveStored,
  );
  expect(state.imports).toEqual([]);
  assertClean(state);
});

test('loading gives shared feedback, rejects duplicates and ignores cancelled out-of-order results', async ({
  page,
}) => {
  const state = await fixtures(page);
  const hold = deferred();
  state.holds.set('Old request', hold);
  state.byQuery.set('Old request', response([{ ...first, title: 'OLD_STALE_RESULT' }]));
  state.byQuery.set('New request', response([{ ...first, title: 'NEW_CURRENT_RESULT' }]));
  await page.goto('/beatmaps/import');
  await runSearch(page, state, 'Old request');
  await expect.poll(() => state.queries.length).toBe(1);
  await expect(state.search.getByRole('status')).toHaveText(state.copy.maps.officialSearch.loading);
  await expect(state.search.locator('.official-search-skeleton')).toBeVisible();
  await expect(page.locator('.navigation-progress')).toHaveAttribute('data-pending', 'true');
  await expect(
    state.search.getByRole('button', { name: state.copy.common.search, exact: true }),
  ).toBeDisabled();
  await state.search
    .locator('form')
    .evaluate((form) =>
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })),
    );
  expect(state.queries).toHaveLength(1);
  await page.locator('#official-search-query').fill('New request');
  await expect(
    state.search.getByRole('button', { name: state.copy.common.search, exact: true }),
  ).toBeEnabled();
  await runSearch(page, state);
  await expect(state.search).toContainText('NEW_CURRENT_RESULT');
  hold.release();
  await expect.poll(() => state.settledQueries.includes('Old request')).toBe(true);
  await expect(state.search).not.toContainText('OLD_STALE_RESULT');
  await expect(state.search.locator('.official-search-skeleton')).toHaveCount(0);
  await expect(page.locator('.navigation-progress')).toHaveAttribute('data-pending', 'false');
  expect(state.queries).toHaveLength(2);
  assertClean(state);
});

test('changing filters removes old results and empty recovery resets without automatic requests', async ({
  page,
}) => {
  const state = await fixtures(page);
  state.outcomes.push(response(), response([]));
  await page.goto('/beatmaps/import');
  await runSearch(page, state);
  await expect(state.search.locator('.official-search-set')).toHaveCount(2);
  await selectField(page.locator('#official-search-status'), 'graveyard');
  await expect(state.search.locator('.official-search-set')).toHaveCount(0);
  expect(state.queries).toHaveLength(1);
  await runSearch(page, state, 'No match');
  await expect(state.search).toContainText(state.copy.maps.officialSearch.emptyTitle);
  await state.search
    .getByRole('button', { name: state.copy.maps.officialSearch.resetFilters, exact: true })
    .click();
  await expect(page.locator('#official-search-query')).toHaveValue('');
  await expect(page.locator('#official-search-query')).toBeFocused();
  await expect(page.locator('#official-search-status')).toHaveAttribute('data-value', 'ranked');
  await expect(state.search.locator('.official-search-empty')).toHaveCount(0);
  expect(state.queries).toHaveLength(2);
  assertClean(state);
});

for (const locale of ['en', 'zh-TW', 'ja'] as const) {
  test(`official search errors provide ${locale} recovery and retain filters for explicit retry`, async ({
    page,
  }) => {
    const state = await fixtures(page, { locale });
    await page.goto('/beatmaps/import');
    await page.locator('#official-search-query').fill('Keep this query');
    const cases = [
      [503, 'official_api_not_configured', 'errorConfig'],
      [502, 'official_auth_failed', 'errorAuth'],
      [502, 'official_search_invalid', 'errorUnavailable'],
      [502, 'official_api_failed', 'errorUnavailable'],
      [503, 'official_search_timeout', 'errorTimeout'],
      [429, 'official_rate_limited', 'errorRateLimit'],
      [429, 'official_resolution_busy', 'errorBusy'],
      [400, 'invalid_request', 'errorQuery'],
      [401, 'unauthorized', 'errorSignIn'],
      [403, 'forbidden', 'errorPermission'],
    ] as const;
    for (const [status, code, key] of cases) {
      state.outcomes.push({ status, code });
      await runSearch(page, state);
      await expect(state.search.getByRole('alert')).toHaveText(state.copy.maps.officialSearch[key]);
      await expect(page.locator('#official-search-query')).toHaveValue('Keep this query');
      await expect(state.search).not.toContainText('RAW_UPSTREAM_ERROR');
    }
    const requests = state.queries.length;
    await state.search.getByRole('button', { name: state.copy.common.retry, exact: true }).click();
    await expect(state.search.locator('.official-search-set')).toHaveCount(2);
    expect(state.queries).toHaveLength(requests + 1);
    expect(state.queries.at(-1)?.get('q')).toBe('Keep this query');
    assertClean(state);
  });
}

test('failed additional page keeps current results and retries the same opaque cursor', async ({
  page,
}) => {
  const state = await fixtures(page);
  const cursor = 'page-two+/=&';
  state.outcomes.push(
    { ...response([first], cursor), total: 2 },
    { status: 429, code: 'official_rate_limited' },
    response([second]),
  );
  await page.goto('/beatmaps/import');
  await runSearch(page, state);
  await expect(state.search.locator('.official-search-set')).toHaveCount(1);
  await state.search
    .getByRole('button', { name: state.copy.maps.officialSearch.loadMore, exact: true })
    .click();
  await expect(state.search.getByRole('alert')).toHaveText(
    state.copy.maps.officialSearch.errorRateLimit,
  );
  await expect(state.search.locator('.official-search-set')).toHaveCount(1);
  await state.search.getByRole('button', { name: state.copy.common.retry, exact: true }).click();
  await expect(state.search.locator('.official-search-set')).toHaveCount(2);
  expect(state.queries.slice(1).map((query) => query.get('cursor_string'))).toEqual([
    cursor,
    cursor,
  ]);
  assertClean(state);
});

test('withdrawn local sets cannot prefill and official download restrictions remain descriptive', async ({
  page,
}) => {
  const state = await fixtures(page);
  state.outcomes.push(
    response([{ ...first, download_disabled: true, local: { ...first.local, withdrawn: true } }]),
  );
  await page.goto('/beatmaps/import');
  await page.locator('#official_id').fill('99100');
  await runSearch(page, state);
  await state.search.locator('.official-search-summary').click();
  await expect(state.search).toContainText(state.copy.maps.officialSearch.withdrawnHint);
  await expect(state.search).toContainText(state.copy.maps.officialSearch.downloadDisabled);
  await expect(
    state.search.getByRole('button', {
      name: state.copy.maps.officialSearch.useDifficulty,
      exact: true,
    }),
  ).toBeDisabled();
  await expect(
    state.search.getByRole('button', { name: state.copy.maps.officialSearch.useSet, exact: true }),
  ).toBeDisabled();
  await expect(page.locator('#official_id')).toHaveValue('99100');
  await expect(
    state.search.getByRole('link', { name: state.copy.maps.officialSetPage, exact: true }),
  ).toHaveAttribute('href', first.official_url);
  expect(state.imports).toEqual([]);
  assertClean(state);
});

test('long difficulty list uses shadcn keyboard selection and contains its menu width', async ({
  page,
}) => {
  const state = await fixtures(page);
  const many = {
    ...first,
    difficulties: Array.from({ length: 70 }, (_, index) => ({
      ...first.difficulties[0],
      official_id: 92000 + index,
      version: `Difficulty ${index + 1} — a deliberately long extended original fixture version name`,
    })),
  };
  state.outcomes.push(response([many]));
  await page.goto('/beatmaps/import');
  await runSearch(page, state);
  const summary = state.search.locator('.official-search-summary');
  await summary.focus();
  await summary.press('Enter');
  const select = page.locator(`#official-search-difficulty-${first.official_set_id}`);
  await select.focus();
  await select.press('Enter');
  await expect(page.getByRole('listbox')).toBeVisible();
  // Radix positions the menu before asynchronously focusing its selected item.
  // Wait for that focus before sending the following keyboard operation.
  await expect(
    page.getByRole('option').and(page.locator('[data-option-value="92000"]')),
  ).toBeFocused();
  await page.keyboard.press('End');
  await expect(
    page.getByRole('option').and(page.locator('[data-option-value="92069"]')),
  ).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(select).toHaveAttribute('data-value', '92069');
  await expect(select).toBeFocused();
  await state.search
    .getByRole('button', { name: state.copy.maps.officialSearch.useDifficulty, exact: true })
    .click();
  await expect(page.locator('#official_id')).toHaveValue('92069');
  await select.focus();
  await select.press('Enter');
  await expect(page.getByRole('listbox')).toBeVisible();
  const box = await page.getByRole('listbox').boundingBox();
  expect(box?.height).toBeLessThanOrEqual(page.viewportSize()!.height - 24);
  expect(box?.width).toBeLessThanOrEqual(page.viewportSize()!.width - 24);
  await page.keyboard.press('Escape');
  await expect(select).toBeFocused();
  assertClean(state);
});

for (const [name, guest, permissions, allowed] of [
  ['guest', true, [], false],
  ['member', false, [], false],
  ['upload-only', false, ['beatmaps.upload'], false],
  ['importer', false, ['beatmaps.import'], true],
  ['operator', false, ['*'], true],
] as const) {
  test(`${name} sees official search only with the import grant`, async ({ page }) => {
    const state = await fixtures(page, { guest, permissions: [...permissions] });
    await page.goto('/beatmaps/import');
    if (allowed) await expect(state.search).toBeVisible();
    else {
      await expect(state.search).toHaveCount(0);
      await expect(
        page.getByRole('heading', {
          name: guest ? state.copy.common.signInRequired : state.copy.common.noPermission,
          exact: true,
        }),
      ).toBeVisible();
    }
    expect(state.queries).toEqual([]);
    assertClean(state);
  });
}

for (const locale of ['en', 'zh-TW', 'ja'] as const) {
  test(`${locale} official search fits desktop and phone in both themes with full glass`, async ({
    page,
  }) => {
    const state = await fixtures(page, { locale });
    await page.goto('/beatmaps/import');
    await runSearch(page, state);
    await state.search.locator('.official-search-summary').first().click();
    for (const [layout, width, height] of [
      ['desktop', 1440, 1000],
      ['mobile', 390, 844],
    ] as const) {
      await page.setViewportSize({ width, height });
      for (const theme of ['dark', 'light'] as const) {
        await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
        await expect(page.locator('html')).toHaveClass(new RegExp(`\\b${theme}\\b`));
        const bounds = await state.search.boundingBox();
        expect(bounds?.width).toBeLessThanOrEqual(width);
        expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
          width,
        );
        await expect(page.locator('#official-search-mode')).toHaveAttribute(
          'data-slot',
          'select-trigger',
        );
        await capture(page, `${layout}-${theme}-${locale}`);
      }
    }
    assertClean(state);
  });
}

test('minimum glass coverage keeps official search solid while retaining the chosen panel opacity', async ({
  page,
}) => {
  const state = await fixtures(page, { glass: 0 });
  await page.goto('/beatmaps/import');
  await expect(state.search).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('data-glass-tier', 'navigation');
  const material = await state.search.evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      background: style.backgroundColor,
      blur: style.backdropFilter,
      opacity: getComputedStyle(document.documentElement)
        .getPropertyValue('--panel-opacity')
        .trim(),
    };
  });
  expect(material.background).not.toMatch(/rgba|\/ 0\.55/);
  expect(material.blur).toBe('blur(0px)');
  expect(Number(material.opacity)).toBeCloseTo(0.55);
  assertClean(state);
});
