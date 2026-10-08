import { expect, test, type Page } from '@playwright/test';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import type { RankHistoryPoint, RankHistoryResponse } from '../src/features/rank-history';
import { selectField } from './helpers/field-select';

// Intercepted synthetic observations only. The parent task verifies the actual collector/API separately.
const playerId = 991801;
const end = '2026-10-08';
const captureNames = [
  '1440-dark-zh-TW-observed',
  '1440-light-en-service-error',
  '390-light-en-country-touch',
  '320-dark-ja-single-unranked',
];
const fixtureDescription =
  'Synthetic intercepted profile and rank-history observations. No database or real account is used.';
const localeCopy = {
  en: {
    title: 'Rank history',
    placement: 'Placement',
    period: 'Period',
    country: 'Country rank',
    table: 'Daily observations',
    empty: 'No observations yet',
    error: 'Rank history could not be loaded. Try again.',
    retry: 'Try again',
    unranked: 'Unranked',
  },
  'zh-TW': {
    title: '排名歷史',
    placement: '排名',
    period: '期間',
    country: '國家排名',
    table: '每日記錄',
    empty: '尚無記錄',
    error: '無法載入排名歷史，請重試。',
    retry: '重試',
    unranked: '未排名',
  },
  ja: {
    title: '順位の履歴',
    placement: '順位',
    period: '期間',
    country: '国別順位',
    table: '日ごとの記録',
    empty: 'まだ記録がありません',
    error: '順位履歴を読み込めませんでした。もう一度お試しください。',
    retry: '再試行',
    unranked: '順位なし',
  },
};
type Locale = keyof typeof localeCopy;

function points(mode = 0, variant = 'vanilla'): RankHistoryPoint[] {
  const offset = mode * 100 + (variant === 'relax' ? 200 : 0);
  return [
    { date: '2026-10-01', pp: 510.25, rank: 120, country_rank: 10, country: 'TW' },
    { date: '2026-10-02', pp: 530.75, rank: 110, country_rank: 9, country: 'TW' },
    { date: '2026-10-04', pp: 550, rank: 100, country_rank: 8, country: 'TW' },
    { date: '2026-10-05', pp: 0, rank: null, country_rank: null, country: 'TW' },
    { date: '2026-10-06', pp: 570, rank: 90, country_rank: 7, country: 'TW' },
    { date: '2026-10-07', pp: 580, rank: 85, country_rank: 3, country: 'JP' },
    { date: end, pp: 600.5, rank: 80, country_rank: 2, country: 'JP' },
  ].map((point) => ({
    ...point,
    pp: point.pp ? point.pp + offset : 0,
    rank: point.rank === null ? null : point.rank + offset,
    captured_at: `${point.date}T23:30:00Z`,
  }));
}
function response(
  mode: number,
  variant: 'vanilla' | 'relax',
  days: number,
  observations = points(mode, variant),
): RankHistoryResponse {
  const start = new Date(`${end}T00:00:00Z`);
  start.setUTCDate(start.getUTCDate() - days + 1);
  return {
    mode,
    variant,
    days,
    start_date: start.toISOString().slice(0, 10),
    end_date: end,
    time_zone: 'UTC',
    collection_interval_seconds: 1800,
    last_captured_at: observations.at(-1)?.captured_at ?? null,
    points: observations,
  };
}
async function fixture(page: Page, locale: Locale = 'en', theme = 'dark') {
  const state = {
    viewer: 991802 as number | null,
    fail: 0,
    shape: 'many' as 'many' | 'single' | 'zero' | 'empty',
    malformed: false,
    hold: false,
    release: null as (() => void) | null,
    queries: [] as { mode: number; variant: string; days: number; viewer: number | null }[],
    writes: 0,
  };
  const player = {
    id: playerId,
    username: 'Synthetic rank history fixture with a deliberately long player name',
    country: 'TW',
    bio: 'Synthetic rank-history observations for interface testing; no real player history.',
    avatar_url: null,
    banner_url: null,
    accent_color: '#78b9dc',
    roles: [],
    permissions: [],
    joined_at: '2026-10-01T00:00:00Z',
    pp: 600.5,
    play_count: 33,
    statistics: { rank: 80, country_rank: 7, accuracy: 98.9 },
  };
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: locale, url: test.info().project.use.baseURL as string },
    ]);
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const resource = url.pathname.replace('/api/v1', '');
    const failure = (status: number) =>
      route.fulfill({
        status,
        json: { error: { code: 'synthetic_failure', message: 'Raw untranslated synthetic error' } },
      });
    if (request.method() !== 'GET' && resource !== '/auth/logout') state.writes++;
    if (resource === '/me')
      return state.viewer === null
        ? failure(401)
        : route.fulfill({ json: { ...player, id: state.viewer, username: 'Synthetic viewer' } });
    if (resource === '/me/preferences')
      return route.fulfill({
        json: {
          mode: theme,
          locale,
          accent_color: '#f28eb6',
          background_url: null,
          background_blur: 0,
          background_dim: 0.76,
          panel_blur: 8,
          panel_opacity: 0.55,
          border_radius: 14,
        },
      });
    if (resource === '/me/relationships')
      return route.fulfill({ json: { friends: [], blocks: [] } });
    if (resource === '/auth/logout') {
      state.viewer = null;
      return route.fulfill({ status: 204 });
    }
    if (resource === `/users/${playerId}`) return route.fulfill({ json: player });
    if (resource === `/users/${playerId}/rank-history`) {
      const mode = Number(url.searchParams.get('mode'));
      const variant = url.searchParams.get('variant') === 'relax' ? 'relax' : 'vanilla';
      const days = Number(url.searchParams.get('days'));
      state.queries.push({ mode, variant, days, viewer: state.viewer });
      if (state.hold)
        await new Promise<void>((resolve) => {
          state.release = resolve;
        });
      if (state.fail) return failure(state.fail);
      const observations =
        state.shape === 'empty'
          ? []
          : state.shape === 'zero'
            ? [points()[3]]
            : state.shape === 'single'
              ? [points(mode, variant).at(-1)!]
              : points(mode, variant);
      const data = response(mode, variant, days, observations);
      if (state.malformed) data.mode = mode === 0 ? 1 : 0;
      return route.fulfill({ json: data });
    }
    return route.fulfill({ json: { items: [], total: 0, page: 1, page_size: 24 } });
  });
  return state;
}
const panel = (page: Page) => page.locator('#profile-rank-history');
async function loaded(page: Page) {
  await expect(panel(page).getByRole('slider')).toBeVisible();
}
async function capture(page: Page, name: string, facts: Record<string, unknown>) {
  await page.evaluate(() => document.fonts.ready);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
  if (process.env.ILLYA_RANK_HISTORY_CAPTURE !== '1') return;
  await page.evaluate(() => {
    document.documentElement.style.setProperty('scroll-behavior', 'auto', 'important');
    document.body.style.setProperty('scroll-behavior', 'auto', 'important');
    (document.activeElement as HTMLElement | null)?.blur();
    window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
  });
  await page.mouse.move(0, 0);
  await expect
    .poll(() =>
      page.evaluate(async () => {
        window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
        await new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        );
        return window.scrollY;
      }),
    )
    .toBe(0);
  const directory = path.resolve('../../.impeccable/review');
  mkdirSync(directory, { recursive: true });
  const filename = `rank-history-${name}.png`;
  await page.screenshot({
    path: path.join(directory, filename),
    fullPage: true,
    animations: 'disabled',
  });
  const record = {
    filename,
    viewport: page.viewportSize(),
    locale: await page.locator('html').getAttribute('lang'),
    theme: await page.locator('html').getAttribute('class'),
    route: new URL(page.url()).pathname + new URL(page.url()).search,
    facts,
  };
  // Each scene has its own record, so a restarted or parallel worker cannot erase another scene.
  writeFileSync(
    path.join(directory, `rank-history-${name}.capture.json`),
    JSON.stringify(record, null, 2),
  );
}

test.afterAll(() => {
  if (process.env.ILLYA_RANK_HISTORY_CAPTURE !== '1') return;
  const directory = path.resolve('../../.impeccable/review');
  mkdirSync(directory, { recursive: true });
  const records = captureNames.flatMap((name) => {
    const filename = path.join(directory, `rank-history-${name}.capture.json`);
    return existsSync(filename) ? [JSON.parse(readFileSync(filename, 'utf8'))] : [];
  });
  writeFileSync(
    path.join(directory, 'rank-history-captures.json'),
    JSON.stringify(
      {
        fixtures: fixtureDescription,
        captures: records,
      },
      null,
      2,
    ),
  );
});

test.use({ timezoneId: 'America/Los_Angeles' });

test('actual observation gaps and unranked days interrupt lines; the full UTC window and better-rank direction remain honest', async ({
  page,
}) => {
  const state = await fixture(page, 'zh-TW');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`/players/${playerId}`);
  await loaded(page);
  await expect(page.locator('.profile-section-nav a[href="#profile-rank-history"]')).toHaveText(
    localeCopy['zh-TW'].title,
  );
  expect(
    await page
      .locator('#profile-rank-history, #profile-about, #profile-scores, #profile-uploads')
      .evaluateAll((elements) => elements.map((element) => element.id)),
  ).toEqual(['profile-rank-history', 'profile-about', 'profile-scores', 'profile-uploads']);
  await expect(page.locator('#profile-about')).toContainText('Synthetic rank-history observations');
  const segments = await panel(page)
    .locator('.rank-history-segment')
    .evaluateAll((elements) =>
      elements.map((element) => [
        element.getAttribute('data-start-date'),
        element.getAttribute('data-end-date'),
      ]),
    );
  expect(segments).toEqual([
    ['2026-10-01', '2026-10-02'],
    ['2026-10-06', '2026-10-07'],
    ['2026-10-07', end],
  ]);
  await expect(panel(page).locator('.rank-history-null')).toHaveAttribute(
    'data-date',
    '2026-10-05',
  );
  const first = panel(page).locator('.rank-history-point[data-date="2026-10-01"]');
  const last = panel(page).locator(`.rank-history-point[data-date="${end}"]`);
  expect(Number(await first.getAttribute('cx'))).toBe(16);
  expect(Number(await last.getAttribute('cx'))).toBe(984);
  expect(Number(await last.getAttribute('cy'))).toBeLessThan(
    Number(await first.getAttribute('cy')),
  );
  await expect(panel(page).locator('.rank-history-dates time').first()).toHaveAttribute(
    'datetime',
    '2026-10-01',
  );
  await expect(panel(page).locator('.rank-history-gap')).toHaveCount(1);
  await expect(panel(page).locator('.rank-history-gap')).toHaveAttribute(
    'data-start-date',
    '2026-10-02',
  );
  await expect(panel(page).locator('.rank-history-gap')).toHaveAttribute(
    'data-end-date',
    '2026-10-04',
  );
  await expect(panel(page).locator('.rank-history-summary time')).toHaveAttribute('datetime', end);
  expect(state.queries.at(-1)).toMatchObject({ mode: 0, variant: 'vanilla', days: 90 });
  await capture(page, '1440-dark-zh-TW-observed', {
    days: 90,
    observationCount: 7,
    missingDate: '2026-10-03',
    unrankedDate: '2026-10-05',
    latestCountry: 'JP',
    profileCountry: 'TW',
    profileAccent: '#78b9dc',
  });
  expect(state.writes).toBe(0);
});

test('country selection uses sampled countries, breaks a country change, and provides keyboard and daily-table point selection in UTC', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto(`/players/${playerId}`);
  await loaded(page);
  await selectField(
    panel(page).getByRole('combobox', { name: 'Placement', exact: true }),
    'country',
  );
  await expect(panel(page).locator('.rank-history-summary dt').nth(1)).toHaveText(
    'Country rank · JP',
  );
  await expect(panel(page).locator('.rank-history-segment')).toHaveCount(2);
  const chart = panel(page).getByRole('slider');
  await chart.focus();
  await chart.press('Home');
  await expect(chart).toHaveAttribute('aria-valuenow', '1');
  await expect(chart).toHaveAttribute('aria-valuetext', /Oct 1, 2026, TW, #10/);
  await chart.press('ArrowRight');
  await expect(chart).toHaveAttribute('aria-valuenow', '2');
  await chart.press('End');
  await chart.press('ArrowLeft');
  await expect(chart).toHaveAttribute('aria-valuenow', '6');
  const disclosureBefore = await panel(page).locator('summary').boundingBox();
  await chart.blur();
  const disclosureAfter = await panel(page).locator('summary').boundingBox();
  expect(disclosureAfter!.y).toBeCloseTo(disclosureBefore!.y, 0);
  await panel(page).locator('summary').click();
  const table = panel(page).getByRole('table');
  await expect(table.getByRole('row')).toHaveCount(8);
  await table.getByRole('button', { name: 'Oct 5, 2026', exact: true }).click();
  await expect(chart).toHaveAttribute('aria-valuetext', /Oct 5, 2026, TW, Unranked, 0 pp/);
  await expect(panel(page).locator('.rank-history-summary dd').nth(2)).toHaveText('0.00 pp');
  await expect(table.locator('time[datetime="2026-10-05T23:30:00Z"]')).toContainText(
    '11:30 PM UTC',
  );
  expect(state.writes).toBe(0);
});

test('timeframe and mode/RX partitions fetch separately without showing previous history while loading; Back restores controls', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto(`/players/${playerId}`);
  await loaded(page);
  await selectField(panel(page).getByRole('combobox', { name: 'Period', exact: true }), '30');
  await expect.poll(() => state.queries.at(-1)?.days).toBe(30);
  await loaded(page);
  await expect(panel(page).locator('.rank-history-dates time').first()).toHaveAttribute(
    'datetime',
    '2026-10-01',
  );
  await selectField(panel(page).getByRole('combobox', { name: 'Period', exact: true }), '180');
  await expect.poll(() => state.queries.at(-1)?.days).toBe(180);
  await loaded(page);
  await page.goBack();
  await expect(panel(page).getByRole('combobox', { name: 'Period', exact: true })).toHaveAttribute(
    'data-value',
    '30',
  );
  await loaded(page);
  state.hold = true;
  await page
    .locator('.profile-mode-rail')
    .getByRole('button', { name: 'taiko', exact: true })
    .click();
  await expect.poll(() => state.release !== null).toBe(true);
  await expect(panel(page).getByRole('slider')).toHaveCount(0);
  await expect(panel(page).getByRole('status')).toBeVisible();
  state.hold = false;
  state.release!();
  state.release = null;
  await loaded(page);
  await expect(panel(page).locator('.rank-history-summary dd').nth(2)).toHaveText('700.50 pp');
  state.hold = true;
  await page
    .locator('.profile-mode-rail')
    .getByRole('button', { name: 'Relax (RX)', exact: true })
    .click();
  await expect.poll(() => state.release !== null).toBe(true);
  await expect(panel(page).locator('.rank-history-summary')).toHaveCount(0);
  state.hold = false;
  state.release!();
  await loaded(page);
  expect(state.queries.at(-1)).toMatchObject({ mode: 1, variant: 'relax', days: 30 });
  await expect(panel(page).locator('.rank-history-summary dd').nth(2)).toHaveText('900.50 pp');
  expect(state.writes).toBe(0);
});

test('empty collection and one ranked observation show no invented history or trend', async ({
  page,
}) => {
  const state = await fixture(page);
  state.shape = 'empty';
  await page.goto(`/players/${playerId}`);
  await expect(panel(page).getByText('No observations yet', { exact: true })).toBeVisible();
  await expect(panel(page).getByRole('slider')).toHaveCount(0);
  await expect(panel(page).getByText('No capture yet.', { exact: false })).toBeVisible();
  state.shape = 'single';
  await selectField(panel(page).getByRole('combobox', { name: 'Period', exact: true }), '30');
  await loaded(page);
  await expect(panel(page).locator('.rank-history-point')).toHaveCount(1);
  await expect(panel(page).locator('.rank-history-point')).toHaveAttribute('cx', '500');
  await expect(panel(page).locator('.rank-history-segment')).toHaveCount(0);
  await expect(panel(page).locator('figcaption')).toContainText(
    'One day recorded. There is no trend line yet.',
  );
  await panel(page).getByRole('slider').press('Home');
  await expect(panel(page).getByRole('slider')).toHaveAttribute('aria-valuenow', '1');
  expect(state.writes).toBe(0);
});

test('localized service failure stays separate from the profile and retries into real observations', async ({
  page,
}) => {
  const state = await fixture(page, 'en', 'light');
  state.fail = 503;
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`/players/${playerId}`);
  await expect(panel(page).getByRole('alert')).toHaveText(/Rank history could not be loaded/);
  await expect(page.locator('#profile-about')).toContainText('Synthetic rank-history observations');
  await expect(page.getByText('Raw untranslated synthetic error')).toHaveCount(0);
  await capture(page, '1440-light-en-service-error', {
    rankHistoryStatus: 503,
    profileStatus: 200,
  });
  state.fail = 0;
  await panel(page).getByRole('button', { name: 'Try again', exact: true }).click();
  await loaded(page);
  await expect(panel(page).getByRole('alert')).toHaveCount(0);
  expect(state.writes).toBe(0);
});

for (const status of [401, 403, 404]) {
  test(`access ${status} clears cached history on return and never restores old points on a failed retry`, async ({
    page,
  }) => {
    const state = await fixture(page);
    await page.goto(`/players/${playerId}`);
    await loaded(page);
    await page.goto('/news');
    state.fail = status;
    await page.goBack();
    await expect(panel(page).getByRole('alert')).toBeVisible();
    await expect(panel(page).getByRole('slider')).toHaveCount(0);
    await expect(panel(page).locator('.rank-history-summary')).toHaveCount(0);
    await panel(page).getByRole('button', { name: 'Try again', exact: true }).click();
    await expect(panel(page).getByRole('alert')).toBeVisible();
    await expect(panel(page).locator('.rank-history-point')).toHaveCount(0);
    state.fail = 0;
    state.shape = 'empty';
    await panel(page).getByRole('button', { name: 'Try again', exact: true }).click();
    await expect(panel(page).getByText('No observations yet', { exact: true })).toBeVisible();
    expect(state.writes).toBe(0);
  });
}

test('a response from another partition is treated as a recoverable error', async ({ page }) => {
  const state = await fixture(page);
  state.malformed = true;
  await page.goto(`/players/${playerId}`);
  await expect(panel(page).getByRole('alert')).toContainText(localeCopy.en.error);
  await expect(panel(page).locator('.rank-history-point')).toHaveCount(0);
  state.malformed = false;
  await panel(page).getByRole('button', { name: 'Try again', exact: true }).click();
  await loaded(page);
});

test('logging out and returning as a guest rechecks access without reusing the previous viewer history', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto(`/players/${playerId}`);
  await loaded(page);
  expect(state.queries.at(-1)?.viewer).toBe(991802);
  state.fail = 404;
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page).toHaveURL(/\/$/);
  await page.goto(`/players/${playerId}`);
  await expect(panel(page).getByRole('alert')).toBeVisible();
  await expect(panel(page).locator('.rank-history-summary')).toHaveCount(0);
  expect(state.queries.at(-1)?.viewer).toBe(null);
  state.fail = 0;
  state.shape = 'empty';
  await panel(page).getByRole('button', { name: 'Try again', exact: true }).click();
  await expect(panel(page).getByText('No observations yet', { exact: true })).toBeVisible();
  expect(state.writes).toBe(0);
});

test.describe('touch and compact localized profiles', () => {
  test.use({ hasTouch: true });
  test('390px light English country history selects by touch and exposes the daily table without document overflow', async ({
    page,
  }) => {
    const state = await fixture(page, 'en', 'light');
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(`/players/${playerId}?rank_days=30&rank_metric=country`);
    await loaded(page);
    const chart = panel(page).locator('.rank-history-chart > svg');
    await chart.scrollIntoViewIfNeeded();
    const bounds = await chart.boundingBox();
    expect(bounds).not.toBeNull();
    await page.touchscreen.tap(bounds!.x + bounds!.width * 0.75, bounds!.y + bounds!.height / 2);
    await expect(panel(page).getByRole('slider')).toHaveAttribute(
      'aria-valuetext',
      /Oct 6, 2026, TW, #7/,
    );
    await panel(page).locator('summary').click();
    await expect(panel(page).getByRole('table')).toBeVisible();
    await expect(
      panel(page).getByRole('region', { name: 'Rank history daily observations' }),
    ).toBeVisible();
    await capture(page, '390-light-en-country-touch', {
      days: 30,
      metric: 'country',
      selectedDate: '2026-10-06',
      selectedCountry: 'TW',
      profileUsername: 'Synthetic long player name',
      observationCount: 7,
    });
    expect(state.writes).toBe(0);
  });
  test('320px dark Japanese zero-pp observation remains selectable and unranked with no line', async ({
    page,
  }) => {
    const state = await fixture(page, 'ja');
    state.shape = 'zero';
    await page.setViewportSize({ width: 320, height: 780 });
    await page.goto(`/players/${playerId}`);
    await loaded(page);
    await expect(
      panel(page).getByRole('heading', { name: localeCopy.ja.title, exact: true }),
    ).toBeVisible();
    await expect(panel(page).locator('.rank-history-summary dd').nth(1)).toHaveText(
      localeCopy.ja.unranked,
    );
    await expect(panel(page).locator('.rank-history-summary dd').nth(2)).toHaveText('0.00 pp');
    await expect(panel(page).locator('.rank-history-point')).toHaveCount(0);
    await expect(panel(page).locator('.rank-history-null')).toHaveCount(1);
    await expect(panel(page).locator('.rank-history-segment')).toHaveCount(0);
    await panel(page).getByRole('slider').press('End');
    await expect(panel(page).getByRole('slider')).toHaveAttribute('aria-valuenow', '1');
    await capture(page, '320-dark-ja-single-unranked', {
      days: 90,
      observationCount: 1,
      selectedDate: '2026-10-05',
      pp: 0,
      rank: null,
      countryRank: null,
    });
    expect(state.writes).toBe(0);
  });
});
