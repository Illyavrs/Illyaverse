import { expect, test, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import english from '../messages/en.json';
import chinese from '../messages/zh-TW.json';
import japanese from '../messages/ja.json';
import type { Beatmap, BeatmapUploadResult } from '../src/lib/types';

// All API requests and upload bytes are intercepted synthetic fixtures. These
// cases do not verify official credentials, real archives or native clients.
const messages = { en: english, 'zh-TW': chinese, ja: japanese };
type Locale = keyof typeof messages;
const setId = 73421;
const archiveFile = {
  name: 'complete-official.osz',
  mimeType: 'application/octet-stream',
  buffer: Buffer.from('PK synthetic complete archive'),
};
const packageId = '0b248fbf-63e3-4f34-a7ad-1ae5bbc728d4';
const members: Beatmap[] = Array.from({ length: 25 }, (_, index) => ({
  id: 96300 + index,
  official_id: 96300 + index,
  title: 'Official archive fixture with a long movement title for wrapping',
  artist: 'Official fixture artist',
  creator: 'Official fixture mapper',
  version: `Difficulty ${index + 1} — extended official version`,
  mode: 0,
  status: index % 2 ? 'loved' : 'ranked',
  source: 'official',
  stars: 4.25,
  length_seconds: 207,
  bpm: 176,
  max_combo: 412,
  play_count: 0,
  md5: (96300 + index).toString(16).padStart(32, '0'),
  download_kind: 'osz',
  download_url: `/api/v1/beatmaps/${96300 + index}/download`,
  package: {
    id: packageId,
    native_set_id: setId,
    source: 'official',
    kind: 'osz',
    legacy: false,
    difficulty_count: 25,
    beatmap_ids: Array.from({ length: 25 }, (_, member) => 96300 + member),
    download_url: '/api/v1/beatmaps/96300/download',
  },
}));
const archiveResult: BeatmapUploadResult = {
  items: members,
  package: members[0].package!,
  idempotent: false,
};
function listing<T>(items: T[], page = 1, pageSize = 24) {
  return {
    items: items.slice((page - 1) * pageSize, page * pageSize),
    page,
    page_size: pageSize,
    total: items.length,
  };
}
function deferred() {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}
type Outcome = { status: number; code: string } | BeatmapUploadResult;
async function fixtures(
  page: Page,
  options: { locale?: Locale; permissions?: string[]; guest?: boolean } = {},
) {
  const locale = options.locale ?? 'en';
  const copy = messages[locale];
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
  const state = {
    singles: [] as unknown[],
    archives: [] as { contentType: string; body: string }[],
    outcomes: [] as Outcome[],
    hold: null as ReturnType<typeof deferred> | null,
    membershipQueries: [] as string[],
    unexpected: [] as string[],
    errors: [] as string[],
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
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const pathname = url.pathname.replace('/api/v1', '');
    const method = request.method();
    if (method === 'GET' && pathname === '/me')
      return route.fulfill(
        options.guest
          ? { status: 401, json: { error: { code: 'unauthorized' } } }
          : {
              json: {
                id: 96200,
                username: 'Official archive fixture',
                country: 'TW',
                bio: '',
                roles: [],
                permissions: options.permissions ?? ['beatmaps.import'],
                avatar_url: null,
                banner_url: null,
                accent_color: '#f28eb6',
                joined_at: '2026-10-08T00:00:00Z',
                pp: 0,
                play_count: 0,
              },
            },
      );
    if (method === 'GET' && pathname === '/me/preferences')
      return route.fulfill({ json: preferences });
    if (method === 'GET' && pathname === '/beatmaps')
      return route.fulfill({ json: listing(members.slice(0, 1)) });
    if (method === 'POST' && pathname === '/beatmaps/import') {
      expect(request.headers()['content-type']).toContain('application/json');
      state.singles.push(request.postDataJSON());
      return route.fulfill({ json: { ...members[0], package: null, download_kind: 'osu' } });
    }
    if (method === 'POST' && pathname === '/beatmaps/import/archive') {
      state.archives.push({
        contentType: request.headers()['content-type'] ?? '',
        body: request.postDataBuffer()?.toString() ?? '',
      });
      await state.hold?.promise;
      const outcome = state.outcomes.shift() ?? archiveResult;
      return route.fulfill(
        'status' in outcome
          ? {
              status: outcome.status,
              json: {
                error: {
                  code: outcome.code,
                  message: 'RAW_OFFICIAL_ERROR_MUST_NOT_REPLACE_TRANSLATION',
                },
              },
            }
          : { json: outcome },
      );
    }
    const detail = /^\/beatmaps\/(\d+)$/.exec(pathname);
    if (method === 'GET' && detail)
      return route.fulfill({
        json: members.find((member) => member.id === Number(detail[1])) ?? members[0],
      });
    const membership = /^\/beatmaps\/\d+\/package\/difficulties$/.test(pathname);
    if (method === 'GET' && membership) {
      state.membershipQueries.push(url.search);
      return route.fulfill({
        json: listing(
          members,
          Number(url.searchParams.get('page')),
          Number(url.searchParams.get('page_size')),
        ),
      });
    }
    if (method === 'GET' && /^\/beatmaps\/\d+\/scores$/.test(pathname))
      return route.fulfill({
        json: {
          ...listing([]),
          mode: 0,
          score_variant: 'vanilla',
          ordered_by: 'standardised_score',
        },
      });
    state.unexpected.push(`${method} ${pathname}`);
    return route.fulfill({ status: 501, json: { error: { code: 'unexpected_fixture_request' } } });
  });
  return Object.assign(state, { copy });
}
function assertClean(state: Awaited<ReturnType<typeof fixtures>>) {
  expect(state.unexpected).toEqual([]);
  expect(state.errors).toEqual([]);
}
async function selectArchive(page: Page, id = String(setId)) {
  await page.locator('#official-import-kind').selectOption('archive');
  await page.locator('#official_set_id').fill(id);
  await page.locator('#official_archive_file').setInputFiles(archiveFile);
}
async function captureReview(page: Page, name: string) {
  if (process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW !== '1') return;
  if (
    process.env.ILLYA_CAPTURE_WORKFLOW_TARGET &&
    process.env.ILLYA_CAPTURE_WORKFLOW_TARGET !== name
  )
    return;
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
      // Cancel any smooth scroll started by the result's focus or paging.
      window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
      await document.fonts.ready;
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      );
      window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      return {
        viewport_width: innerWidth,
        document_width: document.documentElement.scrollWidth,
        scroll_y: window.scrollY,
        theme: document.documentElement.className,
      };
    });
    expect(metrics.document_width).toBeLessThanOrEqual(metrics.viewport_width);
    expect(metrics.scroll_y).toBe(0);
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
          'Intercepted official import fixtures; no real official credentials, package persistence or clients.',
        layouts,
      },
      null,
      2,
    ),
  );
}

test('single-ID JSON import remains the default and switching retains both drafts without stale feedback', async ({
  page,
}) => {
  const state = await fixtures(page);
  await page.goto('/beatmaps/import');
  await expect(page.locator('#official-import-kind')).toHaveValue('difficulty');
  await expect(page.locator('#official_set_id')).toBeHidden();
  await page.locator('#official_id').fill('96300');
  await selectArchive(page);
  await page.locator('#official-import-kind').selectOption('difficulty');
  await expect(page.locator('#official_id')).toHaveValue('96300');
  await page
    .getByRole('button', { name: state.copy.maps.importDifficultyAction, exact: true })
    .click();
  await expect(page.getByRole('status')).toHaveText(state.copy.maps.importSuccess);
  expect(state.singles).toEqual([{ official_id: 96300 }]);
  expect(state.archives).toHaveLength(0);
  await page.locator('#official-import-kind').selectOption('archive');
  await expect(page.getByRole('status')).toHaveCount(0);
  await expect(page.locator('#official_set_id')).toHaveValue(String(setId));
  expect(
    await page
      .locator('#official_archive_file')
      .evaluate((input: HTMLInputElement) => input.files?.[0]?.name),
  ).toBe(archiveFile.name);
  await expect(page.locator('#official_id')).toBeDisabled();
  assertClean(state);
});

test('complete official multipart import and exact retry preserve current statuses and safe result pagination', async ({
  page,
}) => {
  const state = await fixtures(page);
  state.outcomes.push(archiveResult, { ...archiveResult, idempotent: true });
  await page.goto('/beatmaps/import');
  await selectArchive(page);
  await expect(page.locator('#official-archive-hint')).toHaveText(
    state.copy.maps.archiveImportHint,
  );
  await expect(
    page.getByRole('link', { name: state.copy.maps.archiveImportGuide, exact: true }),
  ).toHaveAttribute('href', '/docs/official-archives');
  await page.locator('form.upload-form').evaluate((form) => {
    const events = { invalid: 0, submit: 0 };
    Object.defineProperty(window, '__officialFormEvents', { value: events });
    form.addEventListener('invalid', () => events.invalid++, true);
    form.addEventListener('submit', () => events.submit++, true);
  });
  const submit = page.getByRole('button', {
    name: state.copy.maps.importArchiveAction,
    exact: true,
  });
  await submit.click();
  const result = page.locator('.map-upload-result');
  await expect(result.getByRole('status')).toHaveText(state.copy.maps.importArchiveSuccess);
  await expect(result).toBeFocused();
  await expect(result.locator('.package-difficulty-row')).toHaveCount(24);
  await expect(result.locator('.status-ranked').first()).toHaveText(state.copy.common.ranked);
  await expect(
    result.getByRole('link', { name: state.copy.maps.downloadPackage, exact: true }),
  ).toHaveAttribute('href', archiveResult.package.download_url);
  expect(state.archives).toHaveLength(1);
  expect(state.archives[0].contentType).toContain('multipart/form-data; boundary=');
  expect(state.archives[0].body).toMatch(new RegExp(`name="official_set_id"\\r\\n\\r\\n${setId}`));
  expect(state.archives[0].body).toContain(`name="file"; filename="${archiveFile.name}"`);
  expect(state.archives[0].body).not.toContain('name="official_id"');
  expect(state.singles).toEqual([]);
  const assertPagingOnly = async (submits: number) => {
    const observed = await page.evaluate(() => ({
      ...(window as unknown as { __officialFormEvents: { invalid: number; submit: number } })
        .__officialFormEvents,
      focusedFile: document.activeElement?.id === 'official_archive_file',
    }));
    expect(observed).toEqual({ invalid: 0, submit: submits, focusedFile: false });
    expect(state.archives).toHaveLength(submits);
  };
  await result.getByRole('button', { name: state.copy.common.next, exact: true }).click();
  await expect(result.locator('.package-difficulty-row')).toHaveCount(1);
  await assertPagingOnly(1);
  await result.getByRole('button', { name: state.copy.common.previous, exact: true }).click();
  await assertPagingOnly(1);
  await selectArchive(page);
  await submit.click();
  await expect(result.getByRole('status')).toHaveText(state.copy.maps.importArchiveRetrySuccess);
  await expect(result).toContainText(state.copy.maps.importArchiveRetryHint);
  await expect(result.locator('.status-ranked').first()).toHaveText(state.copy.common.ranked);
  await result.getByRole('button', { name: state.copy.common.next, exact: true }).click();
  await assertPagingOnly(2);
  await result.getByRole('button', { name: state.copy.common.previous, exact: true }).click();
  await assertPagingOnly(2);
  await captureReview(page, 'workflow-official-map-package-retry');
  assertClean(state);
});

test('official archive detail reuses actual osz downloads and independent paged membership', async ({
  page,
}) => {
  const state = await fixtures(page);
  await page.goto(`/beatmaps/${members[24].id}`);
  const panel = page.locator('.map-package');
  await expect(panel.locator('.package-current')).toContainText(members[24].version);
  await expect(panel.locator('.package-difficulty-row')).toHaveCount(24);
  await expect(
    page.getByRole('link', { name: state.copy.maps.downloadPackage, exact: true }),
  ).toHaveAttribute('href', archiveResult.package.download_url);
  await expect(
    page.getByRole('link', { name: state.copy.maps.downloadDifficulty, exact: true }),
  ).toHaveAttribute('href', `/api/v1/beatmaps/${members[24].id}/osu`);
  await panel.getByRole('button', { name: state.copy.common.next, exact: true }).click();
  await expect(panel.locator('.package-difficulty-row')).toHaveCount(1);
  await expect(panel.locator('[aria-current="page"]')).toHaveAttribute(
    'href',
    `/beatmaps/${members[24].id}`,
  );
  // SWR intentionally revalidates on focus. Keep every request recorded while
  // asserting page transitions independently of repeated same-page fetches.
  const allowedPageQueries = ['?page=1&page_size=24', '?page=2&page_size=24'];
  expect(
    state.membershipQueries.filter(
      (query, index, queries) => index === 0 || query !== queries[index - 1],
    ),
  ).toEqual(allowedPageQueries);
  await panel.getByRole('button', { name: state.copy.common.previous, exact: true }).click();
  await expect(panel.locator('.package-difficulty-row')).toHaveCount(24);
  await captureReview(page, 'workflow-official-map-package-members');
  for (const query of state.membershipQueries) expect(allowedPageQueries).toContain(query);
  assertClean(state);
});

test('busy official archive controls and submit ref reject duplicate submissions', async ({
  page,
}) => {
  const state = await fixtures(page);
  const hold = deferred();
  state.hold = hold;
  await page.goto('/beatmaps/import');
  await selectArchive(page);
  await page
    .getByRole('button', { name: state.copy.maps.importArchiveAction, exact: true })
    .click();
  await expect.poll(() => state.archives.length).toBe(1);
  await expect(page.locator('#official-import-kind')).toBeDisabled();
  await expect(page.locator('#official_set_id')).toBeDisabled();
  await expect(page.locator('#official_archive_file')).toBeDisabled();
  await page
    .locator('form.upload-form')
    .evaluate((form) =>
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })),
    );
  expect(state.archives).toHaveLength(1);
  hold.release();
  await expect(page.getByRole('status')).toHaveText(state.copy.maps.importArchiveSuccess);
  await expect(page.locator('#official-import-kind')).toBeEnabled();
  assertClean(state);
});

test('official set ID and archive file bounds block requests and preserve selected input', async ({
  page,
}) => {
  const state = await fixtures(page);
  await page.goto('/beatmaps/import');
  await selectArchive(page);
  const alert = page.locator('form.upload-form').getByRole('alert');
  for (const value of ['', '0', '2000000000', '1.5']) {
    await page.locator('#official_set_id').fill(value);
    expect(
      await page
        .locator('#official_set_id')
        .evaluate((input: HTMLInputElement) => input.checkValidity()),
    ).toBe(false);
    await page
      .locator('form.upload-form')
      .evaluate((form) =>
        form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })),
      );
    await expect(alert).toHaveText(state.copy.maps.errorOfficialSetId);
    await expect(alert).toBeFocused();
  }
  await page.locator('#official_set_id').focus();
  await page
    .locator('form.upload-form')
    .evaluate((form) =>
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })),
    );
  await expect(alert).toBeFocused();
  await page.locator('#official_set_id').fill(String(setId));
  for (const file of [
    { ...archiveFile, name: 'single.osu' },
    { ...archiveFile, buffer: Buffer.alloc(0) },
  ]) {
    await page.locator('#official_archive_file').setInputFiles(file);
    await page
      .getByRole('button', { name: state.copy.maps.importArchiveAction, exact: true })
      .click();
    await expect(alert).toHaveText(state.copy.maps.errorArchiveFile);
  }
  // Allocate transient browser bytes, not a 64 MiB workspace artifact.
  await page.locator('#official_archive_file').evaluate((input: HTMLInputElement) => {
    const transfer = new DataTransfer();
    transfer.items.add(new File([new ArrayBuffer(64 * 1024 * 1024 + 1)], 'oversize.osz'));
    input.files = transfer.files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await page
    .getByRole('button', { name: state.copy.maps.importArchiveAction, exact: true })
    .click();
  await expect(alert).toHaveText(state.copy.maps.errorArchiveTooLarge);
  expect(
    await page
      .locator('#official_archive_file')
      .evaluate((input: HTMLInputElement) => input.files?.[0]?.size),
  ).toBe(64 * 1024 * 1024 + 1);
  expect(state.archives).toEqual([]);
  await selectArchive(page, '1999999999');
  await page
    .getByRole('button', { name: state.copy.maps.importArchiveAction, exact: true })
    .click();
  await expect(page.getByRole('status')).toHaveText(state.copy.maps.importArchiveSuccess);
  expect(state.archives).toHaveLength(1);
  assertClean(state);
});

for (const locale of ['en', 'zh-TW', 'ja'] as const) {
  test(`official archive known errors use ${locale} recovery copy and retain inputs`, async ({
    page,
  }) => {
    const state = await fixtures(page, { locale });
    const cases = [
      [409, 'official_set_version_conflict', 'errorOfficialSetVersion'],
      [409, 'official_archive_conflict', 'errorOfficialArchiveConflict'],
      [409, 'official_archive_manifest_mismatch', 'errorOfficialArchiveManifest'],
      [400, 'official_archive_invalid', 'errorOfficialArchiveInvalid'],
      [413, 'official_archive_too_large', 'errorArchiveTooLarge'],
      [404, 'beatmapset_not_found', 'errorOfficialNotFound'],
      [502, 'checksum_mismatch', 'errorOfficialChecksum'],
      [502, 'official_identity_mismatch', 'errorOfficialIdentity'],
      [429, 'official_rate_limited', 'errorOfficialRateLimit'],
      [503, 'official_api_not_configured', 'errorOfficialAPIConfig'],
      [502, 'official_auth_failed', 'errorOfficialAPIAuth'],
      [502, 'official_api_failed', 'errorOfficialAPIFailed'],
    ] as const;
    await page.goto('/beatmaps/import');
    await selectArchive(page);
    const alert = page.locator('form.upload-form').getByRole('alert');
    for (const [status, code, key] of cases) {
      state.outcomes.push({ status, code });
      await page
        .getByRole('button', { name: state.copy.maps.importArchiveAction, exact: true })
        .click();
      await expect(alert).toHaveText(state.copy.maps[key]);
      await expect(alert).toBeFocused();
      await expect(page.locator('#official_set_id')).toHaveValue(String(setId));
      expect(
        await page
          .locator('#official_archive_file')
          .evaluate((input: HTMLInputElement) => input.files?.[0]?.name),
      ).toBe(archiveFile.name);
      await expect(page.locator('#official-import-kind')).toBeEnabled();
    }
    assertClean(state);
  });
}

for (const [name, guest, permissions, allowed] of [
  ['guest', true, [], false],
  ['member', false, [], false],
  ['local-uploader', false, ['beatmaps.upload'], false],
  ['official-importer', false, ['beatmaps.import'], true],
  ['operator', false, ['*'], true],
] as const) {
  test(`${name} retains distinct upload/import grants for both official flows`, async ({
    page,
  }) => {
    const state = await fixtures(page, { guest, permissions: [...permissions] });
    await page.goto('/beatmaps/import');
    if (allowed) {
      await expect(page.locator('#official-import-kind')).toBeVisible();
      await page.locator('#official-import-kind').selectOption('archive');
      await expect(page.locator('#official_set_id')).toBeVisible();
    } else {
      await expect(page.locator('form.upload-form')).toHaveCount(0);
      await expect(
        page.getByRole('heading', {
          name: guest ? state.copy.common.signInRequired : state.copy.common.noPermission,
          exact: true,
        }),
      ).toBeVisible();
    }
    expect(state.archives).toEqual([]);
    expect(state.singles).toEqual([]);
    assertClean(state);
  });
}
