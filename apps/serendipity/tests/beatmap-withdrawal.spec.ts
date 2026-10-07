import { test, expect, type Page } from '@playwright/test';

// Synthetic browser fixtures. Real publication/retention policy is checked by owned PG tests.
const stamp = '2026-10-08T00:00:00Z';
const fixtureUser = 990501;
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
  };
}
function fixtureMap(index = 0, withdrawn = false) {
  const id = 990600 + index;
  return {
    id,
    native_set_id: index < 2 ? 990700 : 990700 + index,
    official_id: null,
    title: `Withdrawal fixture ${index}`,
    artist: 'Illyaverse fixture',
    creator: 'Fixture mapper',
    version: `Difficulty ${index}`,
    mode: 0,
    status: index === 1 ? 'loved' : 'ranked',
    source: 'local',
    stars: 3.25,
    bpm: 180,
    length_seconds: 120,
    max_combo: 100,
    play_count: 0,
    md5: '0'.repeat(32),
    download_url: `/api/v1/beatmaps/${id}/download`,
    download_kind: 'osu',
    package: null,
    availability: {
      withdrawn,
      revision: withdrawn ? 1 : 0,
      reason: withdrawn ? 'Retained fixture reason' : (null as string | null),
      changed_at: withdrawn ? stamp : (null as string | null),
    },
  };
}
async function capture(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready);
  // Reset before a full-page capture so fixed navigation keeps its real top position.
  await page.evaluate(() => window.scrollTo({ top: 0, left: 0, behavior: 'instant' }));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
  if (process.env.ILLYA_CAPTURE_WITHDRAWAL_REVIEW === '1')
    await page.screenshot({
      path: `../../.impeccable/review/beatmap-withdrawal-${name}.png`,
      fullPage: true,
      animations: 'disabled',
    });
}
async function fixture(
  page: Page,
  locale = 'en',
  mode = 'dark',
  permissions = ['admin.view', 'beatmaps.moderate'],
) {
  const pref = preferences(locale, mode);
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: locale, url: test.info().project.use.baseURL as string },
    ]);
  const state = {
    items: Array.from({ length: 25 }, (_, index) => fixtureMap(index)),
    viewer: fixtureUser as number | null,
    permissions,
    failWrite: false,
    failRefresh: false,
    conflict: false,
    denyDetail: 0,
    queries: [] as string[],
    writes: [] as {
      setId: number;
      withdrawn: boolean;
      expected_revision: number;
      reason: string;
    }[],
    adminReads: 0,
    publicReads: 0,
    holdMe: false,
    releaseMe: null as (() => void) | null,
  };
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname.replace('/api/v1', '');
    const fail = (status: number, code: string) =>
      route.fulfill({ status, json: { error: { code } } });
    if (path === '/me') {
      if (state.holdMe)
        await new Promise<void>((resolve) => {
          state.releaseMe = resolve;
        });
      return state.viewer === null
        ? fail(401, 'unauthorized')
        : route.fulfill({
            json: {
              id: state.viewer,
              username: 'Withdrawal fixture staff',
              country: 'TW',
              bio: '',
              avatar_url: null,
              banner_url: null,
              accent_color: pref.accent_color,
              roles: [],
              permissions: state.permissions,
              joined_at: stamp,
              pp: 0,
              play_count: 0,
            },
          });
    }
    if (path === '/me/preferences') return route.fulfill({ json: pref });
    if (path === '/auth/logout') {
      state.viewer = null;
      return route.fulfill({ status: 204 });
    }
    const change = /^\/admin\/beatmapsets\/(\d+)\/availability$/.exec(path);
    if (change && request.method() === 'PATCH') {
      if (!state.permissions.includes('beatmaps.moderate')) return fail(403, 'forbidden');
      if (state.failWrite) return fail(503, 'unavailable');
      const body = request.postDataJSON();
      const setId = Number(change[1]);
      const members = state.items.filter((map) => map.native_set_id === setId);
      if (state.conflict) {
        state.conflict = false;
        members.forEach((map) => map.availability.revision++);
        return fail(409, 'availability_conflict');
      }
      if (body.expected_revision !== members[0].availability.revision)
        return fail(409, 'availability_conflict');
      state.writes.push({ setId, ...body });
      members.forEach((map) => {
        map.availability = {
          withdrawn: body.withdrawn,
          revision: map.availability.revision + 1,
          reason: body.reason,
          changed_at: stamp,
        };
      });
      return route.fulfill({
        json: { native_set_id: setId, availability: members[0].availability },
      });
    }
    if (path === '/admin/beatmaps') {
      state.queries.push(url.search);
      if (state.failRefresh && state.writes.length) return fail(503, 'unavailable');
      const query = url.searchParams;
      const availability = query.get('availability') ?? 'active';
      const items = state.items.filter(
        (map) =>
          (availability === 'all' ||
            map.availability.withdrawn === (availability === 'withdrawn')) &&
          ['mode', 'source', 'status'].every(
            (key) =>
              !query.has(key) ||
              query.get(key) === String(map[key as 'mode' | 'source' | 'status']),
          ) &&
          (!query.get('q') || map.title.includes(query.get('q')!)),
      );
      const pageNumber = Number(query.get('page') ?? 1);
      return route.fulfill({
        json: {
          items: items.slice((pageNumber - 1) * 24, pageNumber * 24),
          total: items.length,
          page: pageNumber,
          page_size: 24,
        },
      });
    }
    const retained = /^\/admin\/beatmaps\/(\d+)$/.exec(path);
    const publicDetail = /^\/beatmaps\/(\d+)$/.exec(path);
    if (retained || publicDetail) {
      const map = state.items.find((item) => item.id === Number((retained ?? publicDetail)![1]));
      if (retained) {
        state.adminReads++;
        if (state.viewer === null || state.denyDetail)
          return fail(state.denyDetail || 401, 'forbidden');
        if (state.failRefresh && state.writes.length) return fail(503, 'unavailable');
      } else {
        state.publicReads++;
        if (map?.availability.withdrawn) return fail(410, 'beatmap_withdrawn');
      }
      return map ? route.fulfill({ json: map }) : fail(404, 'not_found');
    }
    if (/^\/beatmaps\/\d+\/scores$/.test(path))
      return route.fulfill({
        json: { items: [], total: 0, page: 1, page_size: 24, ordered_by: 'standardised_score' },
      });
    return fail(404, 'fixture_missing');
  });
  return state;
}

test('whole-set withdrawal preserves failed reason and independent ranking statuses', async ({
  page,
}) => {
  const state = await fixture(page, 'zh-TW');
  state.failWrite = true;
  await page.goto('/admin?tab=beatmaps');
  const row = page.locator('tr[data-beatmap-id="990600"]');
  await row.getByRole('button', { name: '下架整組圖譜', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toContainText('同組的所有難度');
  await expect(dialog).toContainText('包含尚未匯入的官方圖譜');
  await dialog.getByLabel('原因', { exact: true }).fill('  管理測試：保留成績與 Replay  ');
  await capture(page, 'desktop-dark-zh-confirm');
  await dialog.getByRole('button', { name: '確認下架', exact: true }).click();
  await expect(dialog.getByRole('alert')).toBeVisible();
  await expect(dialog.getByLabel('原因', { exact: true })).toHaveValue(
    '  管理測試：保留成績與 Replay  ',
  );
  state.failWrite = false;
  await dialog.getByRole('button', { name: '確認下架', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(state.writes).toEqual([
    { setId: 990700, withdrawn: true, expected_revision: 0, reason: '管理測試：保留成績與 Replay' },
  ]);
  expect(state.items.slice(0, 2).map((map) => map.status)).toEqual(['ranked', 'loved']);
  await page.locator('select[name=availability]').selectOption('withdrawn');
  await expect(page.locator('.admin-map-table tbody tr')).toHaveCount(2);
  await expect(row).toContainText('已下架');
});

test('revision conflict requires refresh and retains the reason before retry', async ({ page }) => {
  const state = await fixture(page);
  state.conflict = true;
  await page.goto('/admin?tab=beatmaps');
  await page
    .locator('tr[data-beatmap-id="990600"]')
    .getByRole('button', { name: 'Withdraw set', exact: true })
    .click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Reason', { exact: true }).fill('Check all difficulties');
  await dialog.getByRole('button', { name: 'Confirm withdrawal', exact: true }).click();
  await expect(dialog.getByRole('alert')).toContainText('Another operator');
  await expect(dialog.getByRole('button', { name: 'Confirm withdrawal', exact: true })).toHaveCount(
    0,
  );
  await dialog.getByRole('button', { name: 'Refresh availability', exact: true }).click();
  await expect(dialog).toContainText('Revision 1');
  await expect(dialog.getByLabel('Reason', { exact: true })).toHaveValue('Check all difficulties');
  await dialog.getByRole('button', { name: 'Confirm withdrawal', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(state.writes[0].expected_revision).toBe(1);
});

test('accepted withdrawal hides downloads during failed refresh and cannot be resubmitted', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/beatmaps/990600?management=1');
  await expect(
    page.getByRole('link', { name: 'Download this difficulty .osu', exact: true }),
  ).toHaveCount(1);
  await page.getByRole('button', { name: 'Withdraw set', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Reason', { exact: true }).fill('Temporary availability review');
  state.failRefresh = true;
  await dialog.getByRole('button', { name: 'Confirm withdrawal', exact: true }).click();
  await expect(dialog).toContainText('The set was withdrawn.');
  await expect(dialog).toContainText('The change was saved.');
  await expect(page.locator('.map-downloads')).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: 'Confirm withdrawal', exact: true })).toHaveCount(
    0,
  );
  expect(state.writes).toHaveLength(1);
  state.failRefresh = false;
  await dialog.getByRole('button', { name: 'Refresh availability', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Restore set', exact: true })).toBeEnabled();
  expect(state.writes).toHaveLength(1);
});

test('read-only staff can inspect retained metadata without downloads or actions on Japanese mobile', async ({
  page,
}) => {
  const state = await fixture(page, 'ja', 'light', ['admin.view']);
  state.items[0] = fixtureMap(0, true);
  state.items[1] = fixtureMap(1, true);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/admin?tab=beatmaps&availability=withdrawn');
  await expect(page.locator('.admin-map-table tbody tr')).toHaveCount(2);
  await expect(page.locator('.admin-map-table button')).toHaveCount(0);
  await page.locator('tr[data-beatmap-id="990600"] .score-title').click();
  await expect(page).toHaveURL(
    (url) => url.pathname === '/beatmaps/990600' && url.searchParams.get('management') === '1',
  );
  await expect(page.locator('h1')).toHaveText('Withdrawal fixture 0');
  await expect(page.locator('.map-availability-summary')).toContainText('Retained fixture reason');
  await expect(page.locator('.map-availability-summary')).toContainText('譜面管理権限');
  await expect(page.locator('.map-downloads')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'セットを再公開', exact: true })).toHaveCount(0);
  await capture(page, 'mobile-light-ja-retained');
  expect(state.writes).toHaveLength(0);
});

test('public withdrawn detail shows an explanation without retained metadata or download links', async ({
  page,
}) => {
  const state = await fixture(page, 'en', 'dark', []);
  state.items[0] = fixtureMap(0, true);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/beatmaps/990600');
  await expect(page.getByText('This beatmap set was withdrawn', { exact: true })).toBeVisible();
  await expect(page.locator('.map-detail-cover')).toHaveCount(0);
  await expect(page.locator('.map-downloads')).toHaveCount(0);
  await capture(page, 'mobile-dark-en-public');
  expect(state.adminReads).toBe(0);
});

test('last-page withdrawal clamps page and live filter changes preserve independent state', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/admin?tab=beatmaps&source=local&status=ranked&page=2');
  // Loved remains independent: select all ranking states to expose the 25th row.
  await page.locator('select[name=status]').selectOption('all');
  await page.goto('/admin?tab=beatmaps&source=local&page=2');
  await expect(page.locator('.admin-map-table tbody tr')).toHaveCount(1);
  await page
    .locator('.admin-map-table tbody tr')
    .getByRole('button', { name: 'Withdraw set', exact: true })
    .click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Reason', { exact: true }).fill('Last-page review');
  await dialog.getByRole('button', { name: 'Confirm withdrawal', exact: true }).click();
  await expect.poll(() => Number(new URL(page.url()).searchParams.get('page') ?? 1)).toBe(1);
  await expect(page.locator('.admin-map-table tbody tr')).toHaveCount(24);
  await page.locator('select[name=availability]').selectOption('all');
  await page.locator('select[name=status]').selectOption('ranked');
  expect(new URL(page.url()).searchParams.get('availability')).toBe('all');
  expect(new URL(page.url()).searchParams.get('source')).toBe('local');
  await page.goBack();
  await expect(page.locator('select[name=status]')).toHaveValue('all');
  await expect(page.locator('select[name=availability]')).toHaveValue('all');
  expect(state.writes).toHaveLength(1);
});

test('a cached retained detail is purged after access denial and can recover by explicit retry', async ({
  page,
}) => {
  const state = await fixture(page);
  state.items[0] = fixtureMap(0, true);
  await page.goto('/beatmaps/990600?management=1');
  await expect(page.locator('h1')).toHaveText('Withdrawal fixture 0');
  await page.locator('.back-link').click();
  await expect(page).toHaveURL((url) => url.pathname === '/beatmaps');
  state.denyDetail = 403;
  const denied = page.waitForResponse(
    (response) =>
      response.url().includes('/api/v1/admin/beatmaps/990600') && response.status() === 403,
  );
  await page.goBack();
  await expect(page).toHaveURL(
    (url) => url.pathname === '/beatmaps/990600' && url.searchParams.get('management') === '1',
  );
  await denied;
  await expect(page.locator('.map-detail-cover')).toHaveCount(0);
  await expect(page.locator('.map-availability-summary')).toHaveCount(0);
  state.denyDetail = 0;
  await page.getByRole('button', { name: 'Try again', exact: true }).click();
  await expect(page.locator('h1')).toHaveText('Withdrawal fixture 0');
  expect(state.publicReads).toBe(0);
});

test('logout masks retained management data and does not fall back to a public detail request', async ({
  page,
}) => {
  const state = await fixture(page);
  state.items[0] = fixtureMap(0, true);
  await page.goto('/beatmaps/990600?management=1');
  await expect(page.locator('h1')).toHaveText('Withdrawal fixture 0');
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page).toHaveURL((url) => url.pathname === '/');
  const reads = state.adminReads;
  await page.goBack();
  await expect(page).toHaveURL(
    (url) => url.pathname === '/beatmaps/990600' && url.searchParams.get('management') === '1',
  );
  await expect(page.locator('.map-detail-cover')).toHaveCount(0);
  await expect(page.getByText('Sign in to continue', { exact: true })).toBeVisible();
  expect(state.adminReads).toBe(reads);
  expect(state.publicReads).toBe(0);
});

test('management reads wait for the loaded staff session', async ({ page }) => {
  const state = await fixture(page);
  state.items[0] = fixtureMap(0, true);
  state.holdMe = true;
  await page.goto('/beatmaps/990600?management=1');
  await expect.poll(() => typeof state.releaseMe).toBe('function');
  try {
    await expect(page.locator('.map-detail-cover')).toHaveCount(0);
    expect(state.adminReads).toBe(0);
  } finally {
    state.releaseMe?.();
  }
  await expect(page.locator('h1')).toHaveText('Withdrawal fixture 0');
  expect(state.adminReads).toBe(1);
});

test('restore uses the current revision and preserves every difficulty ranking status', async ({
  page,
}) => {
  const state = await fixture(page);
  state.items[0] = fixtureMap(0, true);
  state.items[1] = fixtureMap(1, true);
  await page.goto('/admin?tab=beatmaps&availability=withdrawn');
  await page
    .locator('tr[data-beatmap-id="990600"]')
    .getByRole('button', { name: 'Restore set', exact: true })
    .click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toContainText('Ranking statuses stay unchanged.');
  await dialog.getByLabel('Reason', { exact: true }).fill('Reviewed and ready');
  await dialog.getByRole('button', { name: 'Confirm restore', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('.admin-map-table')).toHaveCount(0);
  expect(state.writes).toEqual([
    { setId: 990700, withdrawn: false, expected_revision: 1, reason: 'Reviewed and ready' },
  ]);
  expect(state.items.slice(0, 2).map((map) => map.status)).toEqual(['ranked', 'loved']);
});
