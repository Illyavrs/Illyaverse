import { selectField } from './helpers/field-select';
import { test, expect, type Page } from '@playwright/test';
import type { Beatmap } from '../src/lib/types';
import type { DiscoverySet } from '../src/features/map-discovery';

// Intercepted synthetic fixtures only. Real policy/persistence is covered by owned PG tests.
const stamp = '2026-10-08T00:00:00Z';
const owner = 984701;
function preferences(locale = 'en', mode = 'dark') {
  return {
    locale,
    mode,
    accent_color: '#f28eb6',
    background_url: null,
    background_blur: 0,
    background_dim: 0.76,
    panel_blur: 8,
    panel_opacity: 0.55,
    border_radius: 14,
    pm_friends_only: false,
  };
}
function player(locale = 'en', mode = 'dark', id = owner, permissions: string[] = []) {
  return {
    id,
    username: 'Score map fixture player',
    country: 'TW',
    bio: '',
    avatar_url: null,
    banner_url: null,
    accent_color: '#f28eb6',
    joined_at: stamp,
    roles: [],
    permissions,
    preferences: preferences(locale, mode),
  };
}
function map(id = 984702, mode = 0, status = 'ranked', source = 'local'): Beatmap {
  return {
    id,
    official_id: null,
    native_set_id: 2_000_000_000 + id,
    title: `Review fixture map ${id}`,
    artist: 'Illyaverse fixture',
    creator: 'Fixture mapper',
    version: '超難 Fixture',
    mode,
    status,
    source,
    stars: 3.25,
    bpm: 180,
    length_seconds: 120,
    max_combo: 100,
    play_count: 0,
    md5: '0'.repeat(32),
    download_url: `/api/v1/beatmaps/${id}/download`,
    download_kind: 'osu',
    package: null,
  };
}
function score(id = 984703) {
  return {
    id,
    user_id: owner,
    username: 'Score map fixture player',
    beatmap_id: 984702,
    title: 'Replay attachment fixture',
    version: '超難 Fixture',
    mode: 0,
    score_variant: 'vanilla',
    score: 1000000,
    standardised_score: 900000,
    classic_score: 100130,
    score_scale_status: 'legacy_estimated',
    accuracy: 99,
    max_combo: 99,
    mods: 0,
    api_mods: null,
    pp: 123.45,
    pp_eligible: true,
    passed: true,
    verification: 'rejected',
    has_replay: false,
    replay_complete: null,
    created_at: stamp,
    client: 'stable',
    accuracy_source: 'client_reported',
    replay_url: null,
    hit_counts: { n300: 99, n100: 1, misses: 0 },
    user: player(),
    beatmap: map(),
  };
}
function listing<T>(items: T[], page = 1, size = 24) {
  return {
    items: items.slice((page - 1) * size, page * size),
    page,
    page_size: size,
    total: items.length,
  };
}
function discoverySet(item: Beatmap): DiscoverySet {
  return {
    native_set_id: item.native_set_id!,
    representative: item,
    matching_count: 1,
    stored_count: 1,
    matching_modes: [item.mode],
    matching_statuses: [item.status],
    stars_min: item.stars,
    stars_max: item.stars,
    length_min: item.length_seconds,
    length_max: item.length_seconds,
    added_at: stamp,
    preview: [item],
  };
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
      path: `../../.impeccable/review/score-map-${name}.png`,
      fullPage: true,
      animations: 'disabled',
    });
}
async function scoreFixture(
  page: Page,
  locale = 'en',
  theme = 'dark',
  viewer: number | null = owner,
) {
  const pref = preferences(locale, theme);
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: locale, url: test.info().project.use.baseURL as string },
    ]);
  const state = {
    viewer,
    item: score(),
    failUpload: false,
    denyReads: false,
    failReads: false,
    posts: [] as string[],
    accepted: false,
    idempotent: false,
  };
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname.replace('/api/v1', '');
    if (path === '/me')
      return state.viewer === null
        ? route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } })
        : route.fulfill({ json: { ...player(locale, theme, state.viewer), preferences: pref } });
    if (path === '/me/preferences') return route.fulfill({ json: pref });
    if (path === '/auth/logout') {
      state.viewer = null;
      return route.fulfill({ status: 204 });
    }
    if (path === '/scores/984703') {
      if (state.denyReads || (state.viewer === null && state.item.verification !== 'verified'))
        return route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } });
      if (state.failReads && state.accepted)
        return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
      return route.fulfill({ json: state.item });
    }
    if (path === '/scores/984703/replay' && request.method() === 'POST') {
      state.posts.push(request.postDataBuffer()!.toString('utf8'));
      if (state.failUpload)
        return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
      state.accepted = true;
      state.item.has_replay = true;
      state.item.verification = state.idempotent ? 'verified' : 'pending';
      return route.fulfill({
        json: {
          ok: true,
          has_replay: true,
          verification: state.item.verification,
          idempotent: state.idempotent,
        },
      });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'fixture_missing' } } });
  });
  return state;
}

test('owner replay validation and retry preserve selection, then show pending review', async ({
  page,
}) => {
  const state = await scoreFixture(page, 'zh-TW');
  state.failUpload = true;
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/scores/984703');
  await expect(
    page.getByText('此成績已被管理員拒絕，不列入排行榜。', { exact: true }),
  ).toBeVisible();
  const form = page.locator('.score-replay-upload');
  const file = form.locator('input[type=file]');
  await file.setInputFiles({
    name: 'wrong.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('fixture'),
  });
  await form.locator('button[type=submit]').click();
  await expect(form.getByRole('alert')).toBeVisible();
  expect(state.posts).toHaveLength(0);
  await file.setInputFiles({
    name: 'oversized.osr',
    mimeType: 'application/octet-stream',
    buffer: Buffer.alloc(16 * 1024 * 1024 + 1),
  });
  await form.locator('button[type=submit]').click();
  await expect(form.getByRole('alert')).toContainText('16 MiB');
  expect(state.posts).toHaveLength(0);
  await file.setInputFiles({
    name: 'matching-fixture.osr',
    mimeType: 'application/octet-stream',
    buffer: Buffer.from('synthetic-replay'),
  });
  await form.locator('button[type=submit]').click();
  await expect(form.getByRole('alert')).toBeVisible();
  await expect
    .poll(() => file.evaluate((element: HTMLInputElement) => element.files?.[0]?.name))
    .toBe('matching-fixture.osr');
  await capture(page, 'score-desktop-dark-zh-upload-error');
  state.failUpload = false;
  await form.locator('button[type=submit]').click();
  await expect(form.getByRole('status')).toContainText('等待審核');
  await expect(page.locator('.score-performance')).toContainText('待審核');
  await expect(page.locator('.score-performance a')).toHaveAttribute(
    'href',
    '/api/v1/scores/984703/replay',
  );
  expect(state.posts).toHaveLength(2);
  for (const body of state.posts) {
    expect(body).toContain('name="file"; filename="matching-fixture.osr"');
    expect(body).toContain('synthetic-replay');
  }
});

test('accepted upload with failed refresh cannot be sent twice and recovers on refresh', async ({
  page,
}) => {
  const state = await scoreFixture(page, 'en', 'light');
  state.failReads = true;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/scores/984703');
  const section = page.locator('.score-replay-upload');
  await section.locator('input[type=file]').setInputFiles({
    name: 'fixture.osr',
    mimeType: 'application/octet-stream',
    buffer: Buffer.from('fixture'),
  });
  await section.locator('button[type=submit]').click();
  await expect(section.getByRole('status')).toContainText('Replay attached.');
  await expect(section.getByRole('alert')).toContainText('do not upload again');
  await expect(section.locator('button[type=submit]')).toHaveCount(0);
  expect(state.posts).toHaveLength(1);
  await capture(page, 'score-mobile-light-en-refresh-error');
  state.failReads = false;
  await section.getByRole('button', { name: 'Refresh score', exact: true }).click();
  await expect(section.getByRole('alert')).toHaveCount(0);
  expect(state.posts).toHaveLength(1);
});

test('non-owner cannot attach, known and unknown legacy mods never collapse to NM', async ({
  page,
}) => {
  const state = await scoreFixture(page, 'ja', 'dark', owner + 1);
  state.item.verification = 'verified';
  state.item.mods = 32768 + 536870912;
  await page.goto('/scores/984703');
  await expect(page.locator('.score-detail-values')).toContainText('4K SV2');
  await expect(page.locator('.score-replay-upload')).toHaveCount(0);
  state.item.mods = 2147483648;
  await page.reload();
  await expect(page.locator('.score-detail-values')).toContainText('0x80000000');
  await expect(page.locator('.score-detail-values')).not.toContainText('NM');
});

test('policy denial after accepted upload purges private score and accepted feedback', async ({
  page,
}) => {
  const state = await scoreFixture(page);
  await page.goto('/scores/984703');
  const form = page.locator('.score-replay-upload');
  await form.locator('input[type=file]').setInputFiles({
    name: 'fixture.osr',
    mimeType: 'application/octet-stream',
    buffer: Buffer.from('fixture'),
  });
  state.denyReads = true;
  await form.locator('button[type=submit]').click();
  await expect(page.locator('.score-detail-header')).toHaveCount(0);
  await expect(form).toHaveCount(0);
  expect(state.posts).toHaveLength(1);
});

test('sign-out purges a cached private score and its selected replay', async ({ page }) => {
  const state = await scoreFixture(page);
  state.item.verification = 'pending';
  await page.goto('/scores/984703');
  await page.locator('.score-replay-upload input[type=file]').setInputFiles({
    name: 'private.osr',
    mimeType: 'application/octet-stream',
    buffer: Buffer.from('fixture'),
  });
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await page.goto('/scores/984703');
  await expect(page.locator('.score-detail-header')).toHaveCount(0);
  await expect(page.locator('.score-replay-upload')).toHaveCount(0);
  expect(state.posts).toHaveLength(0);
});

async function mapFixture(page: Page, locale = 'en', mode = 'dark', staff = true) {
  const pref = preferences(locale, mode);
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: locale, url: test.info().project.use.baseURL as string },
    ]);
  const state = {
    items: Array.from({ length: 25 }, (_, index) =>
      map(984800 + index, index === 1 ? 2 : 0, 'pending'),
    ),
    failWrite: false,
    failRefresh: false,
    writes: [] as { id: number; status: string }[],
    queries: [] as string[],
  };
  const user = player(locale, mode, owner, staff ? ['admin.view', 'beatmaps.moderate'] : []);
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname.replace('/api/v1', '');
    if (path === '/me') return route.fulfill({ json: user });
    if (path === '/me/preferences') return route.fulfill({ json: pref });
    if (path === '/admin/beatmaps' || path === '/beatmapsets') {
      state.queries.push(path + url.search);
      if (state.failRefresh && state.writes.length)
        return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
      const query = url.searchParams;
      const items = state.items
        .filter(
          (item) =>
            ['mode', 'source', 'status'].every(
              (key) =>
                !query.has(key) ||
                String(item[key as 'mode' | 'source' | 'status']) === query.get(key),
            ) &&
            (!query.get('q') ||
              [item.title, item.artist, item.creator, item.version].some((value) =>
                value.toLowerCase().includes(query.get('q')!.toLowerCase()),
              )),
        )
        .toReversed();
      return route.fulfill({
        json:
          path === '/beatmapsets'
            ? listing(items.map(discoverySet), Number(query.get('page') ?? 1))
            : listing(items, Number(query.get('page') ?? 1)),
      });
    }
    const admin = /^\/admin\/beatmaps\/(\d+)$/.exec(path);
    if (admin && request.method() === 'GET')
      return route.fulfill({
        json: state.items.find((item) => item.id === Number(admin[1])) ?? map(Number(admin[1])),
      });
    if (admin && request.method() === 'PATCH') {
      const write = { id: Number(admin[1]), status: request.postDataJSON().status as string };
      if (state.failWrite)
        return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
      state.writes.push(write);
      state.items.find((item) => item.id === write.id)!.status = write.status;
      return route.fulfill({ json: { ok: true } });
    }
    const detail = /^\/beatmaps\/(\d+)$/.exec(path);
    if (detail)
      return route.fulfill({
        json: state.items.find((item) => item.id === Number(detail[1])) ?? map(Number(detail[1])),
      });
    const siblings = /^\/beatmaps\/(\d+)\/set\/difficulties$/.exec(path);
    if (siblings) {
      const known =
        state.items.find((item) => item.id === Number(siblings[1])) ?? map(Number(siblings[1]));
      return route.fulfill({
        json: listing(
          [known],
          Number(url.searchParams.get('page') ?? 1),
          Number(url.searchParams.get('page_size') ?? 24),
        ),
      });
    }
    if (/^\/beatmaps\/\d+\/scores$/.test(path)) {
      const rx = url.searchParams.get('variant') === 'relax';
      return route.fulfill({
        json: {
          ...listing([
            {
              ...score(984703),
              verification: 'verified',
              username: 'First fixture',
              position: 1,
              score: 100000,
              standardised_score: 950000,
              score_variant: rx ? 'relax' : 'vanilla',
              pp: 400,
              mods: rx ? 128 : 536870912,
            },
            {
              ...score(984704),
              verification: 'verified',
              username: 'Second fixture',
              position: 2,
              score: 9000000,
              standardised_score: 900000,
              score_variant: rx ? 'relax' : 'vanilla',
              pp: 300,
            },
          ]),
          ordered_by: rx ? 'pp' : 'standardised_score',
        },
      });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'fixture_missing' } } });
  });
  return state;
}

test('filtered last-page moderation preserves failed choice and refreshes the correct queue', async ({
  page,
}) => {
  const state = await mapFixture(page);
  state.failWrite = true;
  await page.goto('/admin?tab=beatmaps&source=local&status=pending&page=2');
  const rows = page.locator('.admin-map-table tbody tr');
  await expect(rows).toHaveCount(1);
  const action = rows.first().locator('.admin-map-actions');
  await selectField(action.locator('[data-slot="select-trigger"]'), 'ranked');
  await action.getByRole('button', { name: 'Update status', exact: true }).click();
  await expect(action.getByRole('alert')).toBeVisible();
  await expect(action.locator('[data-slot="select-trigger"]')).toHaveAttribute('data-value', 'ranked');
  expect(state.writes).toHaveLength(0);
  state.failWrite = false;
  await action.getByRole('button', { name: 'Update status', exact: true }).click();
  await expect(rows).toHaveCount(24);
  await expect.poll(() => Number(new URL(page.url()).searchParams.get('page') ?? 1)).toBe(1);
  await expect(page.locator('.admin-map-summary')).toContainText('24 difficulties');
  expect(new URL(page.url()).searchParams.get('source')).toBe('local');
  expect(new URL(page.url()).searchParams.get('status')).toBe('pending');
  await capture(page, 'admin-desktop-dark-en-filtered');
});

test('map search, filter history, review return links and empty Japanese mobile state', async ({
  page,
}) => {
  const state = await mapFixture(page, 'ja', 'light');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/admin?tab=beatmaps');
  const filters = page.locator('.map-review-filters');
  await filters.locator('input[name=q]').fill('超難');
  await filters.locator('button[type=submit]').click();
  await selectField(filters.locator('[data-slot="select-trigger"][data-field-name=mode]'), '2');
  await expect(page.locator('.admin-map-table tbody tr')).toHaveCount(1);
  const returnUrl = page.url();
  await page.locator('.admin-map-table .score-title').click();
  await expect(page.getByRole('link', { name: '管理一覧へ戻る', exact: true })).toHaveAttribute(
    'href',
    new URL(returnUrl).pathname + new URL(returnUrl).search,
  );
  await page.getByRole('link', { name: '管理一覧へ戻る', exact: true }).click();
  await expect(filters.locator('input[name=q]')).toHaveValue('超難');
  await selectField(filters.locator('[data-slot="select-trigger"][data-field-name=source]'), 'official');
  await expect(page.locator('.admin-map-table')).toHaveCount(0);
  await expect(page.getByText('条件に一致する譜面がありません', { exact: true })).toBeVisible();
  await capture(page, 'admin-mobile-light-ja-empty');
  const returnedQueue = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return (
      url.pathname.endsWith('/api/v1/admin/beatmaps') &&
      url.searchParams.get('q') === '超難' &&
      !url.searchParams.has('source')
    );
  });
  await page.goBack();
  await returnedQueue;
  await expect(filters.locator('[data-slot="select-trigger"][data-field-name=source]')).toHaveAttribute('data-value', 'all');
  await expect(page.locator('.admin-map-table tbody tr')).toHaveCount(1);
  await filters.locator('input[name=q]').fill('難'.repeat(40));
  const before = state.queries.length;
  await filters.locator('button[type=submit]').click();
  await expect(filters.getByRole('alert')).toBeVisible();
  expect(state.queries).toHaveLength(before);
});

test('public source selection and board use supplied rank and comparison values', async ({
  page,
}) => {
  const state = await mapFixture(page, 'en', 'dark', false);
  await page.goto('/beatmaps');
  await selectField(page.getByRole('combobox', { name: 'Source', exact: true }), 'local');
  await expect
    .poll(() =>
      state.queries.some(
        (query) => query.startsWith('/beatmapsets?') && query.includes('source=local'),
      ),
    )
    .toBe(true);
  await page.goto('/beatmaps/984702?mode=0&variant=vanilla&returnTo=https%3A%2F%2Fexample.com');
  await expect(page.locator('.back-link')).toHaveAttribute('href', '/beatmaps');
  await expect(page.getByRole('link', { name: 'Manage beatmap', exact: true })).toHaveCount(0);
  const rows = page.locator('.score-table tbody tr');
  await expect(rows).toHaveCount(2);
  await expect(rows.first().locator('.score-board-position')).toHaveText('1');
  await expect(rows.first().locator('.score-board-comparison')).toHaveText('950,000');
  await expect(rows.nth(1).locator('.score-board-comparison')).toHaveText('900,000');
  await expect(rows.first()).toContainText('SV2');
  await capture(page, 'board-desktop-dark-en-order');
  await page.goto('/beatmaps/984702?mode=0&variant=relax');
  await expect(rows.first().locator('.score-board-comparison')).toHaveText('400.00');
  await expect(page.locator('.score-table thead')).not.toContainText('Standardised score');
});
