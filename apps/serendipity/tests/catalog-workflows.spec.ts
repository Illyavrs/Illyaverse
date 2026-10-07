import { expect, test, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';

// All API responses and writes are browser fixtures. Sanctuary's separate
// PostgreSQL test checks that real board rows supply this metadata contract.
const timestamp = '2026-10-07T00:00:00Z';
const player = {
  id: 98501, username: 'Catalogue fixture player', country: 'TW', bio: '',
  roles: [], permissions: [], avatar_url: null, banner_url: null, accent_color: '#f28eb6',
  joined_at: timestamp, pp: 0, play_count: 0,
};
function beatmap(id: number, mode = 0) {
  return {
    id, official_id: null, title: `Catalogue fixture map ${id}`, artist: 'Illyaverse',
    creator: 'Fixture', version: 'Two circles', mode, status: 'ranked', source: 'local',
    stars: 3, bpm: 180, length_seconds: 120, max_combo: 100, play_count: 0, md5: '0'.repeat(32),
  };
}
function score(id: number, map = beatmap(98502), mode = map.mode, relax = false) {
  return {
    id, user_id: player.id, username: player.username, beatmap_id: map.id,
    title: map.title, version: map.version, mode, score_variant: relax ? 'relax' : 'vanilla',
    score: relax ? 0 : 1000000, standardised_score: relax ? null : 1000000,
    accuracy: 99, max_combo: 99, mods: relax ? 128 : 0, pp: relax ? 345 : 123,
    pp_eligible: true, passed: true, verification: 'verified', has_replay: true, created_at: timestamp,
  };
}
const preferences = {
  mode: 'dark', accent_color: '#f28eb6', background_url: null, background_blur: 0,
  background_dim: 0.76, panel_blur: 8, panel_opacity: 0.55, border_radius: 14, locale: 'en',
};
function listing<T>(items: T[], pageNumber: number, size = 24) {
  return {
    items: items.slice((pageNumber - 1) * size, pageNumber * size),
    total: items.length, page: pageNumber, page_size: size,
  };
}
async function english(page: Page) {
  await page.context().addCookies([
    { name: 'illya_locale', value: 'en', url: test.info().project.use.baseURL as string },
  ]);
}
async function captureReview(page: Page, name: string) {
  if (process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW !== '1') return;
  const target = process.env.ILLYA_CAPTURE_WORKFLOW_TARGET;
  if (target && target !== name) return;
  const directory = path.resolve(__dirname, '../../../.impeccable/review');
  mkdirSync(directory, { recursive: true });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const layouts = [];
  for (const [layout, width, height] of [['desktop', 1440, 1000], ['mobile', 390, 844]] as const) {
    await page.setViewportSize({ width, height });
    const metrics = await page.evaluate(async () => {
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      window.scrollTo(0, 0);
      await document.fonts.ready;
      await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
      return {
        viewport_width: innerWidth, document_width: document.documentElement.scrollWidth,
        scroll_y: window.scrollY, rows: document.querySelectorAll('.data-table tbody tr').length,
      };
    });
    expect(metrics.scroll_y).toBe(0);
    expect(metrics.document_width).toBeLessThanOrEqual(metrics.viewport_width);
    const filename = `${name}-${layout}-en.png`;
    await page.screenshot({ path: path.join(directory, filename), fullPage: true, animations: 'disabled' });
    layouts.push({ layout, filename, ...metrics });
  }
  writeFileSync(path.join(directory, `${name}-layout.json`), JSON.stringify({
    provenance: 'Isolated Playwright browser fixtures; no live community data or writes.', layouts,
  }, null, 2));
}

async function moderationFixtures(page: Page, withMaps = false) {
  await english(page);
  const unexpected: string[] = [];
  const browserErrors: string[] = [];
  const queries: string[] = [];
  const writes: { id: number; body: unknown }[] = [];
  let pending = Array.from({ length: 25 }, (_, index) => ({
    ...score(98600 + index), title: `Pending catalogue fixture ${index + 1}`, verification: 'pending',
  }));
  const maps = Array.from({ length: 25 }, (_, index) => beatmap(98700 + index));
  let holdRefresh = false;
  let releaseRefresh: (() => void) | undefined;
  let delayed = false;
  page.on('pageerror', (error) => browserErrors.push(error.message));
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const pathname = url.pathname.replace('/api/v1', '');
    const method = request.method();
    if (method === 'GET' && pathname === '/me') return route.fulfill({ json: {
      ...player, permissions: ['admin.view', 'scores.moderate', ...(withMaps ? ['beatmaps.moderate'] : [])],
    } });
    if (method === 'GET' && pathname === '/me/preferences') return route.fulfill({ json: preferences });
    if (method === 'GET' && ['/scores', '/admin/beatmaps'].includes(pathname)) {
      queries.push(`${pathname}${url.search}`);
      const pageNumber = Number(url.searchParams.get('page'));
      const response = pathname === '/scores' ? listing(pending, pageNumber) : listing(maps, pageNumber);
      if (pathname === '/scores' && pageNumber === 2 && writes.length && holdRefresh) {
        delayed = true;
        await new Promise<void>((resolve) => { releaseRefresh = resolve; });
      }
      return route.fulfill({ json: response });
    }
    if (method === 'PATCH' && /^\/admin\/scores\/\d+$/.test(pathname)) {
      const id = Number(pathname.split('/').at(-1));
      writes.push({ id, body: request.postDataJSON() });
      pending = pending.filter((item) => item.id !== id);
      return route.fulfill({ json: { ok: true } });
    }
    unexpected.push(`${method} ${pathname}${url.search}`);
    return route.fulfill({ status: 501, json: { error: { code: 'unexpected_fixture_request' } } });
  });
  return {
    queries, writes, unexpected, browserErrors,
    delayRefresh: () => { holdRefresh = true; },
    isDelayed: () => delayed,
    releaseRefresh: () => releaseRefresh?.(),
  };
}

for (const [action, verification] of [['Approve', 'verified'], ['Reject', 'rejected']] as const) {
  test(`pending queue returns to page 1 after its final page-2 score is ${verification}`, async ({ page }) => {
    const state = await moderationFixtures(page);
    await page.goto('/admin');
    const scores = page.getByRole('button', { name: 'Scores', exact: true });
    await scores.click();
    await expect(scores).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('.pagination')).toContainText('1 / 2');
    await page.locator('.pagination').getByRole('button', { name: 'Next', exact: true }).click();
    await expect(page.locator('.pagination')).toContainText('2 / 2');
    await expect(page.locator('.data-table tbody tr')).toHaveCount(1);
    const row = page.locator('.data-table tbody tr');
    await expect(row).toContainText('Pending catalogue fixture 25');
    await row.getByRole('button', { name: action, exact: true }).click();
    await expect(page.getByRole('status')).toHaveText('Administrative change saved');
    await expect(page.locator('.data-table tbody tr')).toHaveCount(24);
    await expect(page.locator('.pagination')).toHaveCount(0);
    expect(state.writes).toEqual([{ id: 98624, body: { verification } }]);
    expect(state.queries.at(-1)).toContain('/scores?verification=pending&page=1&page_size=24');
    expect(state.queries.filter((query) => query === '/scores?verification=pending&page=1&page_size=24').length).toBeGreaterThanOrEqual(2);
    expect(state.unexpected).toEqual([]);
    expect(state.browserErrors).toEqual([]);
    if (verification === 'verified') await captureReview(page, 'workflow-catalog-moderation');
  });
}

test('an older score-queue response cannot clamp a different collection on page 2', async ({ page }) => {
  const state = await moderationFixtures(page, true);
  await page.goto('/admin');
  await page.getByRole('button', { name: 'Scores', exact: true }).click();
  await page.locator('.pagination').getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page.locator('.data-table tbody tr')).toHaveCount(1);
  state.delayRefresh();
  await page.getByRole('button', { name: 'Approve', exact: true }).click();
  await expect.poll(state.isDelayed).toBe(true);
  await page.getByRole('button', { name: 'Beatmaps', exact: true }).click();
  await expect(page.locator('.pagination')).toContainText('1 / 2');
  await page.locator('.pagination').getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page.locator('.pagination')).toContainText('2 / 2');
  const response = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return url.pathname === '/api/v1/scores' && url.searchParams.get('page') === '2';
  });
  state.releaseRefresh();
  await (await response).finished();
  await expect(page.locator('.pagination')).toContainText('2 / 2');
  await expect(page.locator('.data-table')).toContainText('Catalogue fixture map 98724');
  expect(state.unexpected).toEqual([]);
  expect(state.browserErrors).toEqual([]);
});

async function boardFixtures(page: Page, map: ReturnType<typeof beatmap>) {
  await english(page);
  const requests: URLSearchParams[] = [];
  const unexpected: string[] = [];
  const browserErrors: string[] = [];
  page.on('pageerror', (error) => browserErrors.push(error.message));
  await page.route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url());
    if (route.request().method() !== 'GET') {
      unexpected.push(`${route.request().method()} ${url.pathname}`);
      return route.fulfill({ status: 501, json: {} });
    }
    if (url.pathname === '/api/v1/me') return route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } });
    if (url.pathname === '/api/v1/me/preferences') return route.fulfill({ json: preferences });
    if (url.pathname === `/api/v1/beatmaps/${map.id}`) return route.fulfill({ json: map });
    if (url.pathname === `/api/v1/beatmaps/${map.id}/scores`) {
      requests.push(url.searchParams);
      const mode = Number(url.searchParams.get('mode'));
      const relax = url.searchParams.get('variant') === 'relax';
      const item = score(98503, map, mode, relax);
      return route.fulfill({ json: {
        ...listing([item], 1), mode, score_variant: item.score_variant,
        ordered_by: relax ? 'pp' : 'standardised_score',
      } });
    }
    unexpected.push(`${route.request().method()} ${url.pathname}${url.search}`);
    return route.fulfill({ status: 501, json: { error: { code: 'unexpected_fixture_request' } } });
  });
  return { requests, unexpected, browserErrors };
}

for (const variant of ['vanilla', 'relax'] as const) {
  test(`${variant} board rows expose map metadata, score details, player partition, and replay links`, async ({ page }) => {
    const map = beatmap(98502);
    const state = await boardFixtures(page, map);
    await page.goto(`/beatmaps/${map.id}?mode=0&variant=${variant}`);
    const row = page.locator('.score-table tbody tr');
    await expect(row.getByRole('link', { name: new RegExp(map.title) })).toHaveAttribute('href', '/scores/98503');
    await expect(row).toContainText('Two circles');
    await expect(row).toContainText(variant === 'relax' ? '345.00' : '123.00');
    await expect(row.getByRole('link', { name: player.username, exact: true })).toHaveAttribute('href', `/players/${player.id}?mode=0&variant=${variant}`);
    await expect(row.locator('a[href$="/replay"]')).toHaveAttribute('href', '/api/v1/scores/98503/replay');
    expect(state.requests.every((query) => query.get('mode') === '0' && query.get('variant') === variant)).toBe(true);
    expect(state.unexpected).toEqual([]);
    expect(state.browserErrors).toEqual([]);
  });
}

for (const [sourceMode, requestedMode, requestedVariant, expectedVariant] of [
  [1, '3', 'relax', 'relax'],
  [2, '4', 'vanilla', 'vanilla'],
  [3, 'unknown', 'relax', 'vanilla'],
] as const) {
  test(`native ruleset ${sourceMode} normalizes unsupported URL modes before requesting a board`, async ({ page }) => {
    const map = beatmap(98510 + sourceMode, sourceMode);
    const state = await boardFixtures(page, map);
    await page.goto(`/beatmaps/${map.id}?mode=${requestedMode}&variant=${requestedVariant}&from=catalogue&page=9`);
    await expect(page.locator('.score-table tbody tr')).toHaveCount(1);
    await expect.poll(() => new URL(page.url()).searchParams.get('mode')).toBe(String(sourceMode));
    expect(new URL(page.url()).searchParams.get('variant')).toBe(expectedVariant);
    expect(new URL(page.url()).searchParams.get('from')).toBe('catalogue');
    expect(new URL(page.url()).searchParams.has('page')).toBe(false);
    const modes = page.getByRole('group', { name: 'Game mode', exact: true });
    for (const [mode, label] of ['osu!', 'taiko', 'catch', 'mania'].entries()) {
      const control = modes.getByRole('button', { name: label, exact: true });
      if (mode === sourceMode) {
        await expect(control).toBeEnabled();
        await expect(control).toHaveAttribute('aria-pressed', 'true');
      } else await expect(control).toBeDisabled();
    }
    if (sourceMode === 3) await expect(page.getByRole('button', { name: 'Relax (RX)', exact: true })).toBeDisabled();
    expect(state.requests.length).toBeGreaterThan(0);
    expect(state.requests.every((query) => query.get('mode') === String(sourceMode) && query.get('variant') === expectedVariant)).toBe(true);
    expect(state.unexpected).toEqual([]);
    expect(state.browserErrors).toEqual([]);
    if (sourceMode === 1) await captureReview(page, 'workflow-native-beatmap');
  });
}

test('standard maps retain conversions and normalize mania to the normal partition', async ({ page }) => {
  const map = beatmap(98502);
  const state = await boardFixtures(page, map);
  await page.goto(`/beatmaps/${map.id}?mode=0&variant=relax`);
  await expect(page.locator('.score-table')).toContainText('345.00');
  const modes = page.getByRole('group', { name: 'Game mode', exact: true });
  for (const [mode, label, variant] of [
    ['1', 'taiko', 'relax'], ['2', 'catch', 'relax'], ['3', 'mania', 'vanilla'], ['0', 'osu!', 'vanilla'],
  ]) {
    const control = modes.getByRole('button', { name: label, exact: true });
    await expect(control).toBeEnabled();
    await control.click();
    await expect.poll(() => new URL(page.url()).searchParams.get('mode')).toBe(mode);
    await expect.poll(() => state.requests.at(-1)?.get('mode')).toBe(mode);
    expect(new URL(page.url()).searchParams.get('variant')).toBe(variant);
    expect(state.requests.at(-1)?.get('variant')).toBe(variant);
    await expect(page.getByRole('button', { name: 'Relax (RX)', exact: true })).toBeEnabled({ enabled: mode !== '3' });
  }
  expect(state.unexpected).toEqual([]);
  expect(state.browserErrors).toEqual([]);
});
