import { expect, test, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import english from '../messages/en.json';
import japanese from '../messages/ja.json';
import chinese from '../messages/zh-TW.json';
import type { BeatmapUploadResult } from '../src/lib/types';

// Every browser API call, including uploads/imports, is intercepted. Real
// package persistence, bytes, fingerprints and conflicts have separate PG tests.
const messages = { en: english, ja: japanese, 'zh-TW': chinese };
type Locale = keyof typeof messages;
const timestamp = '2026-10-07T00:00:00Z';
const packageId = 'cf9beef0-fd90-4d6e-ad88-cb15a94e6ff9';
const longTitle =
  'Original package fixture — a very long title with several movements and an extended finale';

function difficulty(id: number, version: string, status = 'pending') {
  return {
    id,
    title: longTitle,
    artist: 'Original fixture ensemble',
    creator: 'Package fixture mapper',
    version,
    mode: 0,
    status,
    source: 'local',
    stars: 3.75,
    length_seconds: 185,
    bpm: 174,
    max_combo: 321,
  };
}
const members = Array.from({ length: 25 }, (_, index) =>
  difficulty(
    98800 + index,
    `Movement ${String(index + 1).padStart(2, '0')} — extended difficulty name for package navigation`,
  ),
);
function packageMetadata(ids = members.map((member) => member.id), kind: 'osu' | 'osz' = 'osz') {
  return {
    id: packageId,
    native_set_id: 2_000_000_031,
    kind,
    legacy: false,
    difficulty_count: ids.length,
    beatmap_ids: ids,
    download_url: `/api/v1/beatmaps/${ids[0]}/download`,
  };
}
function beatmap(member = members[0], kind: 'osu' | 'osz' = 'osz') {
  return {
    ...member,
    official_id: null,
    official_set_id: null,
    native_id: 2_000_000_080 + member.id - members[0].id,
    native_set_id: 2_000_000_031,
    bpm: 174,
    max_combo: 321,
    play_count: 0,
    md5: member.id.toString(16).padStart(32, '0'),
    download_kind: kind,
    download_url: `/api/v1/beatmaps/${member.id}/download`,
    package: packageMetadata(kind === 'osu' ? [member.id] : undefined, kind),
  };
}
type FixtureMap = Omit<
  ReturnType<typeof beatmap>,
  'package' | 'official_id' | 'official_set_id'
> & {
  package: ReturnType<typeof packageMetadata> | null;
  official_id: number | null;
  official_set_id: number | null;
};
function listing<T>(items: T[], pageNumber: number, pageSize = 24) {
  return {
    items: items.slice((pageNumber - 1) * pageSize, pageNumber * pageSize),
    total: items.length,
    page: pageNumber,
    page_size: pageSize,
  };
}
function deferred() {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}
type UploadOutcome =
  { status: 400 | 409 | 503; code: string } | { items: FixtureMap[]; idempotent: boolean };

async function fixtures(
  page: Page,
  options: {
    locale?: Locale;
    guest?: boolean;
    permissions?: string[];
    map?: FixtureMap;
  } = {},
) {
  const locale = options.locale ?? 'en';
  const copy = messages[locale];
  const primary = options.map ?? beatmap();
  const maps = new Map<number, FixtureMap>(members.map((member) => [member.id, beatmap(member)]));
  maps.set(primary.id, primary);
  const state = {
    maps,
    members: new Map<number, ReturnType<typeof difficulty>[]>(
      members.map((member) => [member.id, members]),
    ),
    queries: [] as { id: number; page: number; pageSize: number }[],
    uploads: [] as { contentType: string; body: Buffer }[],
    imports: [] as unknown[],
    unexpected: [] as string[],
    errors: [] as string[],
    uploadOutcomes: [] as UploadOutcome[],
    memberFailures: new Set<number>(),
    memberHolds: new Map<number, ReturnType<typeof deferred>>(),
    mapFailures: new Set<number>(),
    mapHolds: new Map<number, ReturnType<typeof deferred>>(),
  };
  const player = {
    id: 98901,
    username: 'Package browser fixture',
    country: 'TW',
    bio: '',
    roles: [],
    permissions: options.permissions ?? ['beatmaps.upload', 'beatmaps.import'],
    avatar_url: null,
    banner_url: null,
    accent_color: '#f28eb6',
    joined_at: timestamp,
    pp: 0,
    play_count: 0,
  };
  const preferences = {
    mode: 'system',
    accent_color: '#f28eb6',
    background_url: null,
    background_blur: 0,
    background_dim: 0.76,
    panel_blur: 8,
    panel_opacity: 0.55,
    border_radius: 14,
    locale,
  };
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: locale, url: test.info().project.use.baseURL as string },
    ]);
  await page.addInitScript((value) => {
    localStorage.setItem('illya_appearance', JSON.stringify(value));
  }, preferences);
  page.on('pageerror', (error) => state.errors.push(error.message));
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const pathname = url.pathname.replace('/api/v1', '');
    const method = request.method();
    if (method === 'GET' && pathname === '/me')
      return route.fulfill(
        options.guest
          ? { status: 401, json: { error: { code: 'unauthorized' } } }
          : { json: player },
      );
    if (method === 'GET' && pathname === '/me/preferences')
      return route.fulfill({ json: preferences });
    if (method === 'GET' && pathname === '/beatmaps') {
      return route.fulfill({ json: listing([primary], Number(url.searchParams.get('page') ?? 1)) });
    }
    const detail = /^\/beatmaps\/(\d+)$/.exec(pathname);
    if (method === 'GET' && detail) {
      const id = Number(detail[1]);
      await state.mapHolds.get(id)?.promise;
      if (state.mapFailures.has(id))
        return route.fulfill({ status: 503, json: { error: { code: 'service_unavailable' } } });
      const map = state.maps.get(id);
      return route.fulfill(
        map ? { json: map } : { status: 404, json: { error: { code: 'beatmap_not_found' } } },
      );
    }
    const siblings = /^\/beatmaps\/(\d+)\/package\/difficulties$/.exec(pathname);
    if (method === 'GET' && siblings) {
      const id = Number(siblings[1]);
      const pageNumber = Number(url.searchParams.get('page'));
      const pageSize = Number(url.searchParams.get('page_size'));
      state.queries.push({ id, page: pageNumber, pageSize });
      await state.memberHolds.get(id)?.promise;
      if (state.memberFailures.has(id))
        return route.fulfill({ status: 503, json: { error: { code: 'service_unavailable' } } });
      return route.fulfill({ json: listing(state.members.get(id) ?? [], pageNumber, pageSize) });
    }
    const board = /^\/beatmaps\/(\d+)\/scores$/.exec(pathname);
    if (method === 'GET' && board)
      return route.fulfill({
        json: {
          ...listing([], Number(url.searchParams.get('page') ?? 1)),
          mode: Number(url.searchParams.get('mode')),
          score_variant: url.searchParams.get('variant'),
          ordered_by: 'standardised_score',
        },
      });
    if (method === 'POST' && pathname === '/beatmaps/upload') {
      state.uploads.push({
        contentType: request.headers()['content-type'] ?? '',
        body: request.postDataBuffer() ?? Buffer.alloc(0),
      });
      const outcome = state.uploadOutcomes.shift();
      if (outcome && 'status' in outcome)
        return route.fulfill({
          status: outcome.status,
          json: {
            error: {
              code: outcome.code,
              message: 'RAW_SERVER_MESSAGE_MUST_NOT_REPLACE_LOCALIZED_COPY',
            },
          },
        });
      if (outcome) {
        const result: BeatmapUploadResult = {
          ...outcome,
          package: outcome.items[0].package!,
        };
        return route.fulfill({ json: result });
      }
    }
    if (method === 'POST' && pathname === '/beatmaps/import') {
      state.imports.push(request.postDataJSON());
      return route.fulfill({ json: { ...primary, source: 'official', package: null } });
    }
    state.unexpected.push(`${method} ${pathname}${url.search}`);
    return route.fulfill({ status: 501, json: { error: { code: 'unexpected_fixture_request' } } });
  });
  return { ...state, copy };
}
function assertClean(state: Awaited<ReturnType<typeof fixtures>>) {
  expect(state.unexpected).toEqual([]);
  expect(state.errors).toEqual([]);
}

async function captureReview(page: Page, name: string) {
  if (process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW !== '1') return;
  const target = process.env.ILLYA_CAPTURE_WORKFLOW_TARGET;
  if (target && target !== name) return;
  const directory = path.resolve(__dirname, '../../../.impeccable/review');
  mkdirSync(directory, { recursive: true });
  const layouts = [];
  for (const [layout, width, height, theme] of [
    ['desktop', 1440, 1000, 'dark'],
    ['desktop', 1440, 1000, 'light'],
    ['mobile', 390, 844, 'dark'],
    ['mobile', 390, 844, 'light'],
  ] as const) {
    await page.setViewportSize({ width, height });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
    await expect(page.locator('html')).toHaveClass(new RegExp(`\\b${theme}\\b`));
    const metrics = await page.evaluate(async () => {
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      window.scrollTo(0, 0);
      await document.fonts.ready;
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      );
      return {
        viewport_width: innerWidth,
        document_width: document.documentElement.scrollWidth,
        scroll_y: window.scrollY,
        theme: document.documentElement.className,
      };
    });
    expect(metrics.scroll_y).toBe(0);
    expect(metrics.document_width).toBeLessThanOrEqual(metrics.viewport_width);
    const filename = `${name}-${layout}-${theme}-en.png`;
    await page.screenshot({
      path: path.join(directory, filename),
      fullPage: true,
      animations: 'disabled',
    });
    layouts.push({ layout, filename, ...metrics });
  }
  writeFileSync(
    path.join(directory, `${name}-layout.json`),
    JSON.stringify(
      {
        provenance:
          'Isolated browser fixtures with intercepted API calls; no real accounts, package bytes or writes.',
        layouts,
      },
      null,
      2,
    ),
  );
}

const uploadFile = {
  name: 'original-package-fixture.osz',
  mimeType: 'application/octet-stream',
  buffer: Buffer.from('Original synthetic package payload; intercepted browser API fixture only.'),
};
async function selectedFile(page: Page) {
  return page.locator('#mapfile').evaluate((input) => {
    const file = (input as HTMLInputElement).files?.[0];
    return file ? { name: file.name, size: file.size } : null;
  });
}

test('typed new upload and exact retry expose existing IDs, current statuses and paged difficulty links', async ({
  page,
}) => {
  const state = await fixtures(page);
  const uploaded = members.map((member) => beatmap(member));
  const existing = uploaded.map((map) => ({ ...map, status: 'ranked' }));
  state.uploadOutcomes.push(
    { items: uploaded, idempotent: false },
    { items: existing, idempotent: true },
  );
  await page.goto('/beatmaps/upload');
  await expect(page.getByText(state.copy.maps.uploadPackageHint, { exact: true })).toBeVisible();
  await page.locator('#mapfile').setInputFiles(uploadFile);
  await page.getByRole('button', { name: state.copy.maps.upload, exact: true }).click();
  const result = page.locator('.map-upload-result');
  await expect(
    result.getByRole('heading', { name: state.copy.maps.uploadResultTitle, exact: true }),
  ).toBeVisible();
  await expect(result.getByRole('status')).toHaveText(state.copy.maps.uploadSuccess);
  await expect(result.locator('.package-difficulty-row')).toHaveCount(24);
  const first = result.locator(`a[href="/beatmaps/${members[0].id}"]`);
  await expect(first).toContainText(members[0].version);
  await expect(first).toContainText(state.copy.common.pending);
  await expect(result).toContainText('25 difficulties');
  expect(await selectedFile(page)).toBeNull();
  await page.evaluate(() => {
    const form = document.querySelector('.upload-form');
    if (!form) throw new Error('Upload result must remain inside its form');
    const events = { invalid: 0, submit: 0 };
    Object.defineProperty(window, '__packageFormEvents', { value: events });
    // Native invalid events do not bubble. Observe them without preventing the
    // browser behavior so a missing pagination button type cannot be hidden.
    form.addEventListener(
      'invalid',
      () => {
        events.invalid += 1;
      },
      true,
    );
    form.addEventListener(
      'submit',
      () => {
        events.submit += 1;
      },
      true,
    );
  });
  const assertPaginationOnly = async (expectedSubmits: number) => {
    const events = await page.evaluate(() => ({
      ...(window as unknown as { __packageFormEvents: { invalid: number; submit: number } })
        .__packageFormEvents,
      focusedFile: document.activeElement?.id === 'mapfile',
    }));
    expect(events).toEqual({ invalid: 0, submit: expectedSubmits, focusedFile: false });
  };
  await result.getByRole('button', { name: state.copy.common.next, exact: true }).click();
  await expect(result.locator('.package-difficulty-row')).toHaveCount(1);
  await expect(result.locator('.package-difficulty-row')).toHaveAttribute(
    'href',
    `/beatmaps/${members[24].id}`,
  );
  await expect(result.locator('.pagination')).toContainText('2 / 2');
  await assertPaginationOnly(0);
  expect(state.uploads).toHaveLength(1);
  await result.getByRole('button', { name: state.copy.common.previous, exact: true }).click();
  await expect(result.locator('.package-difficulty-row')).toHaveCount(24);
  await expect(result.locator('.pagination')).toContainText('1 / 2');
  await assertPaginationOnly(0);
  expect(state.uploads).toHaveLength(1);
  // Preserve the existing regression that a new response resets page 2 to 1.
  await result.getByRole('button', { name: state.copy.common.next, exact: true }).click();
  await expect(result.locator('.package-difficulty-row')).toHaveCount(1);
  await assertPaginationOnly(0);
  await page.locator('#mapfile').setInputFiles(uploadFile);
  await page.getByRole('button', { name: state.copy.maps.upload, exact: true }).click();
  await expect(result.getByRole('status')).toHaveText(state.copy.maps.uploadRetrySuccess);
  await expect(result).toContainText(state.copy.maps.uploadRetryHint);
  await expect(result.locator('.package-difficulty-row')).toHaveCount(24);
  await expect(result.locator('.pagination')).toContainText('1 / 2');
  await expect(first).toContainText(state.copy.common.ranked);
  await expect(result).not.toContainText(state.copy.common.pending);
  await expect(
    result.getByRole('link', { name: state.copy.maps.downloadPackage, exact: true }),
  ).toHaveAttribute('href', packageMetadata().download_url);
  expect(await selectedFile(page)).toBeNull();
  await result.getByRole('button', { name: state.copy.common.next, exact: true }).click();
  await expect(result.locator('.package-difficulty-row')).toHaveCount(1);
  await expect(result.locator('.pagination')).toContainText('2 / 2');
  await assertPaginationOnly(1);
  await result.getByRole('button', { name: state.copy.common.previous, exact: true }).click();
  await expect(result.locator('.package-difficulty-row')).toHaveCount(24);
  await expect(result.locator('.pagination')).toContainText('1 / 2');
  await assertPaginationOnly(1);
  expect(state.uploads).toHaveLength(2);
  for (const request of state.uploads) {
    expect(request.contentType).toMatch(/^multipart\/form-data; boundary=/);
    expect(request.body.includes(uploadFile.buffer)).toBe(true);
    expect(request.body.toString()).toContain(`filename="${uploadFile.name}"`);
  }
  await captureReview(page, 'workflow-map-package-retry');
  assertClean(state);
});

for (const locale of ['en', 'zh-TW', 'ja'] as const) {
  test(`known official-ID and package-conflict upload failures use ${locale} copy and retain the selected file`, async ({
    page,
  }) => {
    const state = await fixtures(page, { locale });
    if (locale !== 'en') await page.setViewportSize({ width: 390, height: 844 });
    state.uploadOutcomes.push(
      { status: 400, code: 'official_beatmap_requires_import' },
      { status: 409, code: 'map_package_conflict' },
    );
    await page.goto('/beatmaps/upload');
    await page.locator('#mapfile').setInputFiles(uploadFile);
    for (const key of ['errorOfficialUpload', 'errorPackageConflict'] as const) {
      await page.getByRole('button', { name: state.copy.maps.upload, exact: true }).click();
      await expect(page.locator('.upload-form').getByRole('alert')).toHaveText(
        state.copy.maps[key],
      );
      await expect(page.locator('.upload-form')).not.toContainText(
        'RAW_SERVER_MESSAGE_MUST_NOT_REPLACE_LOCALIZED_COPY',
      );
      expect(await selectedFile(page)).toEqual({
        name: uploadFile.name,
        size: uploadFile.buffer.length,
      });
      await expect(page.locator('#mapfile')).toBeEnabled();
      await expect(page.locator('.map-upload-result')).toHaveCount(0);
      await expect(page.locator('.map-upload-error a[href="/beatmaps/import"]')).toHaveCount(
        key === 'errorOfficialUpload' ? 1 : 0,
      );
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    expect(state.uploads).toHaveLength(2);
    assertClean(state);
  });
}

test('an official-ID upload error respects missing import permission instead of offering an inaccessible CTA', async ({
  page,
}) => {
  const state = await fixtures(page, { permissions: ['beatmaps.upload'] });
  state.uploadOutcomes.push({ status: 400, code: 'official_beatmap_requires_import' });
  await page.goto('/beatmaps/upload');
  await page.locator('#mapfile').setInputFiles(uploadFile);
  await page.getByRole('button', { name: state.copy.maps.upload, exact: true }).click();
  await expect(page.locator('.upload-form').getByRole('alert')).toHaveText(
    state.copy.maps.errorOfficialUpload,
  );
  await expect(
    page.getByText(state.copy.maps.officialImportPermissionHint, { exact: true }),
  ).toBeVisible();
  await expect(page.locator('.map-upload-error a[href="/beatmaps/import"]')).toHaveCount(0);
  expect(await selectedFile(page)).toEqual({
    name: uploadFile.name,
    size: uploadFile.buffer.length,
  });
  assertClean(state);
});

test('a service upload failure preserves the file and the same form can retry successfully', async ({
  page,
}) => {
  const state = await fixtures(page);
  const singleFile = {
    name: 'original-difficulty-fixture.osu',
    mimeType: 'text/plain',
    buffer: Buffer.from('osu file format v14\n[Metadata]\nBeatmapID:-1\nBeatmapSetID:-1\n'),
  };
  state.uploadOutcomes.push(
    { status: 503, code: 'service_unavailable' },
    { items: [beatmap(members[0], 'osu')], idempotent: false },
  );
  await page.goto('/beatmaps/upload');
  await page.locator('#mapfile').setInputFiles(singleFile);
  await page.getByRole('button', { name: state.copy.maps.upload, exact: true }).click();
  await expect(page.locator('.upload-form').getByRole('alert')).toHaveText(
    state.copy.common.serviceError,
  );
  expect(await selectedFile(page)).toEqual({
    name: singleFile.name,
    size: singleFile.buffer.length,
  });
  await expect(page.locator('.map-upload-result')).toHaveCount(0);
  await page.getByRole('button', { name: state.copy.maps.upload, exact: true }).click();
  await expect(page.locator('.map-upload-result').getByRole('status')).toHaveText(
    state.copy.maps.uploadSuccess,
  );
  await expect(page.locator('.upload-form').getByRole('alert')).toHaveCount(0);
  await expect(
    page
      .locator('.map-upload-result')
      .getByRole('link', { name: state.copy.maps.downloadDifficulty, exact: true }),
  ).toHaveAttribute('href', `/api/v1/beatmaps/${members[0].id}/download`);
  await expect(page.locator('.map-upload-result')).toContainText(state.copy.maps.singleFileHint);
  expect(state.uploads).toHaveLength(2);
  expect(await selectedFile(page)).toBeNull();
  assertClean(state);
});

test('a 25-difficulty package pages complete membership and identifies the current difficulty', async ({
  page,
}) => {
  const current = members[24];
  const state = await fixtures(page, { map: beatmap(current) });
  await page.goto(`/beatmaps/${current.id}`);
  const section = page.locator('.map-package');
  await expect(
    section.getByRole('heading', { name: state.copy.maps.packageTitle, exact: true }),
  ).toBeVisible();
  await expect(section).toContainText('25 difficulties');
  await expect(section.locator('.package-difficulty-row')).toHaveCount(24);
  await expect(section.locator('[aria-current="page"]')).toHaveCount(0);
  await expect(section.locator('.pagination')).toContainText('1 / 2');
  await expect(
    page.getByRole('link', { name: state.copy.maps.downloadPackage, exact: true }),
  ).toHaveAttribute('href', `/api/v1/beatmaps/${members[0].id}/download`);
  await expect(
    page.getByRole('link', { name: state.copy.maps.downloadDifficulty, exact: true }),
  ).toHaveAttribute('href', `/api/v1/beatmaps/${current.id}/osu`);
  await expect(page.getByText(state.copy.maps.singleFileHint, { exact: true })).toBeVisible();
  await captureReview(page, 'workflow-map-package-members');
  await section.getByRole('button', { name: state.copy.common.next, exact: true }).click();
  await expect(section.locator('.pagination')).toContainText('2 / 2');
  const row = section.locator('.package-difficulty-row');
  await expect(row).toHaveCount(1);
  await expect(row).toContainText(current.version);
  await expect(row).toContainText(state.copy.maps.currentDifficulty);
  await expect(row).toHaveAttribute('href', `/beatmaps/${current.id}`);
  await expect(row).toHaveAttribute('aria-current', 'page');
  await expect(
    section.getByRole('button', { name: state.copy.common.next, exact: true }),
  ).toBeDisabled();
  await section.getByRole('button', { name: state.copy.common.previous, exact: true }).click();
  await expect(section.locator('.package-difficulty-row')).toHaveCount(24);
  await expect(section.locator('.pagination')).toContainText('1 / 2');
  expect(state.queries).toContainEqual({ id: current.id, page: 1, pageSize: 24 });
  expect(state.queries).toContainEqual({ id: current.id, page: 2, pageSize: 24 });
  assertClean(state);
});

test('sibling navigation resets package paging and never renders the old page while the new one loads', async ({
  page,
}) => {
  const state = await fixtures(page);
  await page.goto(`/beatmaps/${members[0].id}`);
  const section = page.locator('.map-package');
  await expect(section.locator('.package-difficulty-row')).toHaveCount(24);
  await section.getByRole('button', { name: state.copy.common.next, exact: true }).click();
  await expect(section.locator('.package-difficulty-row')).toHaveCount(1);
  const destination = members[24].id;
  const hold = deferred();
  state.memberHolds.set(destination, hold);
  await page.evaluate((id) => {
    const seen: string[] = [];
    Object.defineProperty(window, '__packageTransitions', { value: seen });
    new MutationObserver(() => {
      if (location.pathname !== `/beatmaps/${id}`) return;
      for (const row of document.querySelectorAll('.map-package .package-difficulty-row')) {
        seen.push(row.textContent ?? '');
      }
    }).observe(document.body, { childList: true, subtree: true });
  }, destination);
  try {
    await section.locator(`a[href="/beatmaps/${destination}"]`).click();
    await expect(page).toHaveURL(new RegExp(`/beatmaps/${destination}(?:\\?|$)`));
    await expect
      .poll(() => state.queries.filter((query) => query.id === destination).length)
      .toBeGreaterThan(0);
    await expect(
      section.getByRole('status', { name: state.copy.common.loading, exact: true }),
    ).toBeVisible();
    await expect(section.locator('.package-difficulty-row')).toHaveCount(0);
    await expect(section.locator('.pagination')).toHaveCount(0);
    hold.release();
    await expect(section.locator('.package-difficulty-row')).toHaveCount(24);
    await expect(section.locator('.pagination')).toContainText('1 / 2');
    await expect(section.locator('.package-difficulty-list')).not.toContainText(
      members[24].version,
    );
    const seen = await page.evaluate(
      () => (window as unknown as { __packageTransitions: string[] }).__packageTransitions,
    );
    expect(seen.some((text) => text.includes(members[24].version))).toBe(false);
    expect(
      state.queries
        .filter((query) => query.id === destination)
        .every((query) => query.page === 1 && query.pageSize === 24),
    ).toBe(true);
  } finally {
    hold.release();
  }
  assertClean(state);
});

test('package loading and failure retain the parent map and expose a working retry', async ({
  page,
}) => {
  const state = await fixtures(page);
  const hold = deferred();
  state.memberHolds.set(members[0].id, hold);
  state.memberFailures.add(members[0].id);
  try {
    await page.goto(`/beatmaps/${members[0].id}`);
    const section = page.locator('.map-package');
    await expect(page.getByRole('heading', { name: longTitle, exact: true })).toBeVisible();
    await expect(
      section.getByRole('status', { name: state.copy.common.loading, exact: true }),
    ).toBeVisible();
    await expect(section.locator('.package-difficulty-row')).toHaveCount(0);
    hold.release();
    await expect(section.getByRole('alert')).toBeVisible();
    await expect(page.getByRole('heading', { name: longTitle, exact: true })).toBeVisible();
    state.memberFailures.delete(members[0].id);
    await section.getByRole('button', { name: state.copy.common.retry, exact: true }).click();
    await expect(section.locator('.package-difficulty-row')).toHaveCount(24);
    await expect(section.getByRole('alert')).toHaveCount(0);
    expect(state.queries.length).toBeGreaterThanOrEqual(2);
  } finally {
    hold.release();
  }
  assertClean(state);
});

test('a standalone package clearly downloads one osu difficulty without music or other assets', async ({
  page,
}) => {
  const map = beatmap(members[0], 'osu');
  const state = await fixtures(page, { map });
  state.members.set(map.id, [members[0]]);
  await page.goto(`/beatmaps/${map.id}`);
  await expect(
    page.getByRole('link', { name: state.copy.maps.downloadDifficulty, exact: true }),
  ).toHaveAttribute('href', map.download_url);
  await expect(page.getByText(state.copy.maps.singleFileHint, { exact: true })).toBeVisible();
  await expect(
    page.getByRole('link', { name: state.copy.maps.downloadPackage, exact: true }),
  ).toHaveCount(0);
  await expect(page.locator('.map-package .package-difficulty-row')).toHaveCount(1);
  await expect(page.locator('.map-package [aria-current="page"]')).toHaveCount(1);
  await expect(page.locator('.map-package .pagination')).toHaveCount(0);
  assertClean(state);
});

for (const kind of ['osu', 'osz'] as const) {
  test(`official package-null detail uses the actual ${kind} download kind without requesting local membership`, async ({
    page,
  }) => {
    const map: FixtureMap = {
      ...beatmap(members[0], kind),
      source: 'official',
      official_id: 654321,
      official_set_id: 456789,
      native_id: 654321,
      native_set_id: 456789,
      package: null,
    };
    const state = await fixtures(page, { map });
    await page.goto(`/beatmaps/${map.id}`);
    await expect(page.getByRole('heading', { name: longTitle, exact: true })).toBeVisible();
    await expect(
      page.getByRole('link', {
        name: state.copy.maps[kind === 'osu' ? 'downloadDifficulty' : 'downloadPackage'],
        exact: true,
      }),
    ).toHaveAttribute('href', map.download_url);
    await expect(page.getByText(state.copy.maps.singleFileHint, { exact: true })).toBeVisible();
    if (kind === 'osz')
      await expect(
        page.getByRole('link', { name: state.copy.maps.downloadDifficulty, exact: true }),
      ).toHaveAttribute('href', `/api/v1/beatmaps/${map.id}/osu`);
    await expect(page.locator('.map-package')).toHaveCount(0);
    expect(state.queries).toEqual([]);
    assertClean(state);
  });
}

for (const [name, guest, permissions] of [
  ['guest', true, []],
  ['member', false, []],
  ['uploader', false, ['beatmaps.upload']],
  ['importer', false, ['beatmaps.import']],
  ['operator', false, ['*']],
] as const) {
  test(`${name} sees only accessible upload and import entry points and forms`, async ({
    page,
  }) => {
    const state = await fixtures(page, { guest, permissions: [...permissions] });
    const upload =
      !guest && permissions.some((permission) => ['*', 'beatmaps.upload'].includes(permission));
    const imported =
      !guest && permissions.some((permission) => ['*', 'beatmaps.import'].includes(permission));
    await page.goto('/beatmaps');
    await expect(page.locator('.page-heading a[href="/beatmaps/upload"]')).toHaveCount(
      upload ? 1 : 0,
    );
    await expect(page.locator('.page-heading a[href="/beatmaps/import"]')).toHaveCount(
      imported ? 1 : 0,
    );
    await page.goto('/beatmaps/upload');
    await expect(page.locator('#mapfile')).toHaveCount(upload ? 1 : 0);
    if (!upload)
      await expect(
        page.getByRole('heading', {
          name: state.copy.common[guest ? 'signInRequired' : 'noPermission'],
          exact: true,
        }),
      ).toBeVisible();
    await page.goto('/beatmaps/import');
    await expect(page.locator('#official_id')).toHaveCount(imported ? 1 : 0);
    if (!imported)
      await expect(
        page.getByRole('heading', {
          name: state.copy.common[guest ? 'signInRequired' : 'noPermission'],
          exact: true,
        }),
      ).toBeVisible();
    expect(state.uploads).toEqual([]);
    expect(state.imports).toEqual([]);
    assertClean(state);
  });
}
