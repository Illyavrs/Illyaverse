import { expect, test, type Page } from '@playwright/test';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import english from '../messages/en.json';
import chinese from '../messages/zh-TW.json';
import japanese from '../messages/ja.json';
import { selectField } from './helpers/field-select';

// Profile/account responses and preference writes are intercepted fixtures.
// Country flags are genuine self-hosted static assets; no live account is edited.
const messages = { en: english, 'zh-TW': chinese, ja: japanese };
type Locale = keyof typeof messages;
const playerId = 97601;
const nativeNames = { en: 'English', 'zh-TW': '繁體中文', ja: '日本語' };
async function fixture(
  page: Page,
  options: { locale?: Locale; country?: string; guest?: boolean } = {},
) {
  const locale = options.locale ?? 'en';
  const preferences = {
    mode: 'system',
    locale,
    accent_color: '#f28eb6',
    background_url: null,
    background_blur: 0,
    background_dim: 0.76,
    panel_blur: 8,
    panel_opacity: 0.55,
    glass_level: 100,
    border_radius: 14,
  };
  const user = {
    id: playerId,
    username: 'Country and language fixture',
    country: options.country ?? 'TW',
    bio: '',
    avatar_url: null,
    banner_url: null,
    banner_position_x: 50,
    banner_position_y: 50,
    accent_color: '#f28eb6',
    roles: [],
    permissions: [],
    joined_at: '2026-10-09T00:00:00Z',
    pp: 0,
    play_count: 0,
  };
  const state = {
    patches: [] as unknown[],
    fail: false,
    hold: null as Promise<void> | null,
    errors: [] as string[],
    unexpected: [] as string[],
    flagRequests: [] as string[],
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
  page.on('request', (request) => {
    if (new URL(request.url()).pathname.startsWith('/flags/twemoji/'))
      state.flagRequests.push(new URL(request.url()).pathname);
  });
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const pathname = url.pathname.replace('/api/v1', '');
    if (pathname === '/me')
      return route.fulfill(
        options.guest ? { status: 401, json: { error: { code: 'unauthorized' } } } : { json: user },
      );
    if (pathname === '/me/preferences' && request.method() === 'GET')
      return route.fulfill({ json: preferences });
    if (pathname === '/me/preferences' && request.method() === 'PATCH') {
      const patch = request.postDataJSON();
      state.patches.push(patch);
      await state.hold;
      if (state.fail)
        return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
      Object.assign(preferences, patch);
      return route.fulfill({ json: preferences });
    }
    if (pathname === `/users/${playerId}`) return route.fulfill({ json: user });
    if (pathname === '/me/relationships')
      return route.fulfill({ json: { friends: [], blocks: [] } });
    if (
      pathname === '/beatmaps' ||
      pathname === '/scores' ||
      pathname === `/users/${playerId}/scores/best`
    )
      return route.fulfill({
        json: {
          items: [],
          total: 0,
          page: 1,
          page_size: 24,
          mode: Number(url.searchParams.get('mode') ?? '0'),
          score_variant: url.searchParams.get('variant') ?? 'vanilla',
          collection: 'best',
          ordered_by: 'pp',
        },
      });
    if (pathname === `/users/${playerId}/rank-history`) {
      const days = Number(url.searchParams.get('days') ?? '90');
      const end = new Date('2026-10-09T00:00:00Z');
      const start = new Date(end);
      start.setUTCDate(start.getUTCDate() - days + 1);
      return route.fulfill({
        json: {
          mode: Number(url.searchParams.get('mode') ?? '0'),
          variant: url.searchParams.get('variant') ?? 'vanilla',
          days,
          start_date: start.toISOString().slice(0, 10),
          end_date: end.toISOString().slice(0, 10),
          time_zone: 'UTC',
          collection_interval_seconds: 1800,
          last_captured_at: null,
          points: [],
        },
      });
    }
    if (pathname === '/leaderboard')
      return route.fulfill({
        json: {
          items: ['TW', 'US', 'JP', 'tw', 'ZZ'].map((country, index) => ({
            id: playerId + index,
            rank: index + 1,
            username: `Country fixture ${country}`,
            country,
            avatar_url: null,
            pp: 500 - index,
            accuracy: 99,
            play_count: 10,
          })),
          total: 5,
          page: 1,
          page_size: 24,
        },
      });
    state.unexpected.push(`${request.method()} ${pathname}`);
    return route.fulfill({ status: 501, json: { error: { code: 'fixture_unexpected' } } });
  });
  return Object.assign(state, { copy: messages[locale], preferences });
}
function assertClean(state: Awaited<ReturnType<typeof fixture>>) {
  expect(state.unexpected).toEqual([]);
  expect(state.errors).toEqual([]);
}
async function capture(page: Page, name: string, kind: 'language-menu' | 'profile-flag') {
  if (process.env.ILLYA_CAPTURE_COUNTRY_LANGUAGE !== '1') return;
  const directory = path.resolve(__dirname, '../../../.impeccable/review');
  mkdirSync(directory, { recursive: true });
  await page.evaluate(async () => {
    // Preserve the current menu and keyboard focus while cancelling focus scroll.
    window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
    await document.fonts.ready;
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
    );
    window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  });
  const metrics = await page.evaluate(() => ({
    width: innerWidth,
    height: innerHeight,
    document_width: document.documentElement.scrollWidth,
    scroll_y: window.scrollY,
    locale: document.documentElement.lang,
    theme: document.documentElement.className,
    glass_level: document.documentElement.dataset.glassLevel,
    menu_open: !!document.querySelector('.language-menu-content'),
    profile_country:
      document.querySelector('.profile-player-name .country-flag')?.getAttribute('data-country') ??
      null,
  }));
  expect(metrics.document_width).toBeLessThanOrEqual(metrics.width);
  expect(metrics.scroll_y).toBe(0);
  const filename = `country-language-${name}.png`;
  const bytes = await page.screenshot({
    path: path.join(directory, filename),
    fullPage: true,
    animations: 'disabled',
  });
  const sourceFiles = [
    'src/components/shell.tsx',
    'src/components/country-flag.tsx',
    'src/features/players.tsx',
    'src/styles/country-language.css',
    'src/app/globals.css',
    'public/flags/twemoji/provenance.json',
    'tests/country-language.spec.ts',
  ];
  const source_sha256 = Object.fromEntries(
    sourceFiles.map((file) => [
      file,
      createHash('sha256')
        .update(readFileSync(path.resolve(__dirname, '..', file)))
        .digest('hex'),
    ]),
  );
  writeFileSync(
    path.join(directory, `country-language-${name}.json`),
    JSON.stringify(
      {
        provenance:
          'Synthetic intercepted profile/account/preference fixtures. Genuine self-hosted Twemoji SVGs; no live account writes or native-client verification.',
        kind,
        filename,
        screenshot_sha256: createHash('sha256').update(bytes).digest('hex'),
        source_sha256,
        twemoji_version: '17.0.3',
        twemoji_commit: 'b6b55fef1e8636b540a6d016a4729ca8cdf2e60b',
        ...metrics,
      },
      null,
      2,
    ),
  );
}

for (const locale of ['en', 'zh-TW', 'ja'] as const) {
  test(`${locale} language dropdown retains full native names and a stable scrollbar on desktop and phone`, async ({
    page,
  }) => {
    const state = await fixture(page, { locale });
    await page.goto(`/players/${playerId}`);
    const trigger = page.locator('.language-select');
    await expect(trigger).toHaveAttribute('data-value', locale);
    for (const width of [1440, 390, 320]) {
      await page.setViewportSize({ width, height: 640 });
      for (const theme of ['dark', 'light'] as const) {
        await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
        await expect(page.locator('html')).toHaveClass(new RegExp(`\\b${theme}\\b`));
        const before = await page.evaluate(() => ({
          width: document.documentElement.clientWidth,
          x: document.querySelector('main')!.getBoundingClientRect().x,
          overflow: getComputedStyle(document.body).overflow,
        }));
        await trigger.focus();
        await trigger.press('Enter');
        const menu = page.getByRole('menu');
        await expect(menu).toBeVisible();
        await expect(menu.getByRole('menuitemradio')).toHaveCount(3);
        for (const name of Object.values(nativeNames))
          await expect(menu.getByRole('menuitemradio', { name, exact: true })).toBeVisible();
        await expect(
          menu.getByRole('menuitemradio', { name: nativeNames[locale], exact: true }),
        ).toHaveAttribute('aria-checked', 'true');
        await expect(
          menu.getByRole('menuitemradio', { name: nativeNames[locale], exact: true }),
        ).toBeFocused();
        const bounds = await menu.boundingBox();
        expect(bounds!.x).toBeGreaterThanOrEqual(0);
        expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width);
        const after = await page.evaluate(() => ({
          width: document.documentElement.clientWidth,
          x: document.querySelector('main')!.getBoundingClientRect().x,
          overflow: getComputedStyle(document.body).overflow,
        }));
        expect(after).toEqual(before);
        await expect(page.locator('body')).not.toHaveAttribute('data-scroll-locked');
        await capture(page, `menu-${width}-${theme}-${locale}`, 'language-menu');
        await page.keyboard.press('Escape');
        await expect(menu).toHaveCount(0);
        await expect(trigger).toBeFocused();
      }
    }
    if (locale === 'zh-TW' && process.env.ILLYA_CAPTURE_COUNTRY_LANGUAGE === '1') {
      await page.setViewportSize({ width: 1280, height: 800 });
      await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
      await expect(page.locator('html')).toHaveClass(/\bdark\b/);
      await trigger.focus();
      await trigger.press('Enter');
      await expect(page.getByRole('menu')).toBeVisible();
      await capture(page, 'menu-1280-dark-zh-TW', 'language-menu');
      await page.keyboard.press('Escape');
    }
    expect(state.patches).toEqual([]);
    assertClean(state);
  });
}

for (const [country, code, present] of [
  ['TW', 'TW', true],
  ['US', 'US', true],
  ['JP', 'JP', true],
  ['tw', 'TW', true],
  ['ZZ', 'ZZ', false],
  ['../TW', '../TW', false],
] as const) {
  test(`profile country ${country} uses a self-hosted flag or safe text fallback`, async ({
    page,
  }) => {
    const state = await fixture(page, { country });
    if (process.env.ILLYA_CAPTURE_COUNTRY_LANGUAGE === '1')
      await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
    await page.goto(`/players/${playerId}`);
    const flag = page.locator('.profile-player-name .country-flag');
    await expect(flag).toHaveText(code);
    const image = flag.locator('img');
    if (present) {
      await expect(image).toHaveAttribute('src', `/flags/twemoji/${code}.svg`);
      await expect
        .poll(() =>
          image.evaluate(
            (element: HTMLImageElement) => element.complete && element.naturalWidth > 0,
          ),
        )
        .toBe(true);
      await expect(image).toHaveAttribute(
        'alt',
        new Intl.DisplayNames(['en'], { type: 'region' }).of(code)!,
      );
      await expect(image).toHaveCSS('width', '24px');
      await expect(image).toHaveCSS('height', '18px');
      expect(state.flagRequests).toContain(`/flags/twemoji/${code}.svg`);
      await capture(page, `profile-${country}-dark-en`, 'profile-flag');
    } else {
      await expect(image).toHaveCount(0);
      expect(state.flagRequests).toEqual([]);
    }
    assertClean(state);
  });
}

test('rankings reuse the same country presentation and profile flags survive an unavailable asset', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/rankings');
  await expect(page.locator('.rankings-table tbody .country-flag')).toHaveCount(5);
  await expect(page.locator('.rankings-table tbody .country-flag img')).toHaveCount(4);
  await page.route('**/flags/twemoji/TW.svg', (route) => route.abort());
  await page.goto(`/players/${playerId}`);
  const flag = page.locator('.profile-player-name .country-flag');
  await expect(flag).toHaveText('TW');
  await expect(flag.locator('img')).toHaveCount(0);
  assertClean(state);
});

for (const guest of [false, true]) {
  test(`${guest ? 'guest cookie' : 'account and cookie'} language preference uses the existing persistence flow`, async ({
    page,
  }) => {
    const state = await fixture(page, { guest });
    await page.goto(`/players/${playerId}`);
    await selectField(page.locator('.language-select'), 'ja');
    await expect(page.locator('.language-select')).toHaveAttribute('data-value', 'ja');
    await expect(page.locator('.language-select')).toBeFocused();
    expect(state.patches).toEqual(guest ? [] : [{ locale: 'ja' }]);
    await expect
      .poll(
        async () =>
          (await page.context().cookies()).find((cookie) => cookie.name === 'illya_locale')?.value,
      )
      .toBe('ja');
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
    await expect(page.locator('.language-select')).toHaveAttribute('data-value', 'ja');
    assertClean(state);
  });
}

test('language saving blocks duplicates and failure retains the old locale with ordinary recovery feedback', async ({
  page,
}) => {
  const state = await fixture(page);
  let release!: () => void;
  state.hold = new Promise<void>((resolve) => {
    release = resolve;
  });
  state.fail = true;
  await page.goto(`/players/${playerId}`);
  const trigger = page.locator('.language-select');
  await trigger.click();
  await page.getByRole('menuitemradio', { name: '日本語', exact: true }).click();
  await expect.poll(() => state.patches.length).toBe(1);
  await expect(trigger).toBeDisabled();
  expect(state.patches).toEqual([{ locale: 'ja' }]);
  release();
  await expect(page.locator('.shell-error')).toHaveText(new RegExp(state.copy.common.serviceError));
  await expect(trigger).toBeEnabled();
  await expect(trigger).toBeFocused();
  await expect(trigger).toHaveAttribute('data-value', 'en');
  expect(
    (await page.context().cookies()).find((cookie) => cookie.name === 'illya_locale')?.value,
  ).toBe('en');
  state.fail = false;
  state.hold = null;
  await selectField(trigger, 'ja');
  await expect(trigger).toHaveAttribute('data-value', 'ja');
  await expect(trigger).toBeFocused();
  await expect(page.locator('.shell-error')).toHaveCount(0);
  assertClean(state);
});

test('country asset provenance matches every shipped SVG and the public attribution and license', async ({
  page,
}) => {
  const directory = path.resolve(__dirname, '../public/flags/twemoji');
  const manifest = JSON.parse(readFileSync(path.join(directory, 'provenance.json'), 'utf8')) as {
    commit: string;
    version: string;
    license: string;
    license_sha256: string;
    files: { local: string; sha256: string; byte_length: number; git_blob: string }[];
  };
  expect(manifest.commit).toBe('b6b55fef1e8636b540a6d016a4729ca8cdf2e60b');
  expect(manifest.version).toBe('17.0.3');
  expect(manifest.license).toBe('CC-BY-4.0');
  expect(manifest.files).toHaveLength(259);
  for (const file of manifest.files) {
    const bytes = readFileSync(path.join(directory, file.local));
    expect(bytes.length).toBe(file.byte_length);
    expect(createHash('sha256').update(bytes).digest('hex')).toBe(file.sha256);
    expect(
      createHash('sha1')
        .update(Buffer.from(`blob ${bytes.length}\0`))
        .update(bytes)
        .digest('hex'),
    ).toBe(file.git_blob);
  }
  const license = await page.request.get('/flags/twemoji/LICENSE-GRAPHICS');
  expect(license.ok()).toBe(true);
  expect(
    createHash('sha256')
      .update(await license.body())
      .digest('hex'),
  ).toBe(manifest.license_sha256);
  const attribution = await page.request.get('/flags/twemoji/ATTRIBUTION.txt');
  expect(attribution.ok()).toBe(true);
  expect(await attribution.text()).toContain('Creative Commons Attribution 4.0');
  expect(await attribution.text()).toContain(manifest.commit);
});
