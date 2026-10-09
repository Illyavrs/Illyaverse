import { expect, test, type Locator, type Page } from '@playwright/test';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

// Synthetic browser fixtures only; no account or community mutation is sent.
const playerId = 989940;
const stamp = '2026-10-09T00:00:00Z';
const empty = { items: [], total: 0, page: 1, page_size: 24 };
async function expectNoPaintedShadow(element: Locator) {
  await expect
    .poll(() =>
      element.evaluate((node) => {
        const shadow = getComputedStyle(node).boxShadow;
        if (shadow === 'none') return true;
        // Tailwind's shadow-none can serialize transparent ring/shadow slots. They
        // paint nothing; any opaque slot would still fail this visual-state check.
        const colors = shadow.match(/rgba?\([^)]*\)/g);
        return Boolean(
          colors?.length &&
          colors.every(
            (color) =>
              color.startsWith('rgba(') &&
              Number(color.slice(color.lastIndexOf(',') + 1, -1)) === 0,
          ),
        );
      }),
    )
    .toBe(true);
}
async function fixture(page: Page, theme: 'dark' | 'light') {
  await page
    .context()
    .addCookies([{ name: 'illya_locale', value: 'en', url: test.info().project.use.baseURL! }]);
  await page.addInitScript((mode) => {
    localStorage.setItem(
      'illya_appearance',
      JSON.stringify({
        mode,
        accent_color: '#f28eb6',
        background_url: null,
        background_blur: 0,
        background_dim: 0.76,
        panel_blur: 8,
        panel_opacity: 0.55,
        glass_level: 100,
        border_radius: 14,
        locale: 'en',
      }),
    );
  }, theme);
  await page.route('**/api/v1/**', (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.replace('/api/v1', '');
    if (path === '/me')
      return route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } });
    if (path === '/status')
      return route.fulfill({
        json: {
          players_online: 0,
          registered_players: 0,
          ranked_scores: 0,
          beatmap_count: 0,
        },
      });
    if (path === `/users/${playerId}`)
      return route.fulfill({
        json: {
          id: playerId,
          username: 'Tab fixture',
          country: 'TW',
          bio: '',
          avatar_url: null,
          banner_url: null,
          accent_color: '#f28eb6',
          roles: ['player'],
          permissions: [],
          joined_at: stamp,
          pp: 12.34,
          play_count: 2,
          statistics: {
            mode: Number(url.searchParams.get('mode') ?? '0'),
            score_variant: url.searchParams.get('variant') ?? 'vanilla',
            pp: 12.34,
            rank: 1,
            country_rank: 1,
            accuracy: 98.5,
            play_count: 2,
            maximum_combo: 100,
            total_hits: 200,
            ranked_score: 2000,
            total_score: 3000,
            score_totals_complete: true,
            score_scale: 'classic_display',
            grade_counts: { ssh: 0, ss: 0, sh: 0, s: 1, a: 1, b: 0, c: 0, d: 0, f: 0 },
          },
        },
      });
    if (path.endsWith('/rank-history'))
      return route.fulfill({
        json: {
          mode: Number(url.searchParams.get('mode') ?? '0'),
          variant: url.searchParams.get('variant') ?? 'vanilla',
          days: 90,
          start_date: '2026-07-12',
          end_date: '2026-10-09',
          time_zone: 'UTC',
          collection_interval_seconds: 1800,
          last_captured_at: null,
          points: [],
        },
      });
    return route.fulfill({ json: empty });
  });
}

async function capture(page: Page, name: string, state: string) {
  if (process.env.ILLYA_WEB_SMOKE_CAPTURE !== '1') return;
  await page.evaluate(() => document.fonts.ready);
  await expect(page.locator('.navigation-progress')).toHaveAttribute('data-pending', 'false');
  await expect
    .poll(() => page.locator('.navigation-progress').getAttribute('data-visible'))
    .not.toBe('true');
  await page.evaluate(async () => {
    document.documentElement.style.setProperty('scroll-behavior', 'auto', 'important');
    window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
    );
  });
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
  // Resolve from this test's directory, independent of the command's working directory.
  const review = path.resolve(path.dirname(__filename), '../../../.impeccable/review');
  mkdirSync(review, { recursive: true });
  const output = path.join(review, name);
  const screenshot = await page.screenshot({
    path: `${output}.png`,
    fullPage: true,
    animations: 'disabled',
  });
  const app = path.resolve(path.dirname(__filename), '..');
  const sourceFiles = [
    'src/components/ui/tabs.tsx',
    'src/features/home.tsx',
    'src/features/home.css',
    'src/features/profile.css',
    'src/features/players.tsx',
    'src/app/globals.css',
    'src/app/layout.tsx',
    'src/components/providers.tsx',
    'src/styles/country-language.css',
    'messages/en.json',
    'public/images/home-character.png',
  ];
  writeFileSync(
    `${output}.json`,
    JSON.stringify(
      {
        capturedAt: new Date().toISOString(),
        url: page.url(),
        viewport: page.viewportSize(),
        fullPage: true,
        theme: await page.locator('html').getAttribute('class'),
        locale: 'en',
        reducedMotion: await page.evaluate(
          () => matchMedia('(prefers-reduced-motion: reduce)').matches,
        ),
        state,
        provenance:
          'Synthetic API fixtures intercepted in this Playwright browser; guest appearance only. The character is the original user-supplied PNG, not generated artwork. Capture is from the top after fonts and navigation settle; no real account or community data is modified.',
        screenshotSha256: createHash('sha256').update(screenshot).digest('hex'),
        sources: Object.fromEntries(
          sourceFiles.map((file) => [
            file,
            createHash('sha256')
              .update(readFileSync(path.join(app, file)))
              .digest('hex'),
          ]),
        ),
      },
      null,
      2,
    ),
    'utf8',
  );
}

for (const theme of ['dark', 'light'] as const) {
  for (const width of [1440, 1280, 390, 320]) {
    test(`hero layering and readable wordmark at ${width}px ${theme}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 1000 });
      await page.emulateMedia({ reducedMotion: 'reduce' });
      await fixture(page, theme);
      await page.goto('/');
      await expect(page.locator('html')).toHaveClass(new RegExp(theme));
      const image = page.locator('.home-character-image');
      await expect
        .poll(() => image.evaluate((element) => (element as HTMLImageElement).complete))
        .toBe(true);
      await page.evaluate(() => document.fonts.ready);
      const geometry = await page.evaluate(() => {
        const material = document.querySelector('.home-introduction')!;
        const card = material.getBoundingClientRect();
        const art = document.querySelector('.home-hero-character')!.getBoundingClientRect();
        const image = document.querySelector<HTMLImageElement>('.home-character-image')!;
        const box = image.getBoundingClientRect();
        const head = {
          left: box.left + (219 / 700) * box.width,
          right: box.left + (435 / 700) * box.width,
          top: box.top + (101 / 1251) * box.height,
          bottom: box.top + (321 / 1251) * box.height,
        };
        const title = document.querySelector('.home-intro-copy h1')!;
        const range = document.createRange();
        range.selectNodeContents(title);
        const text = range.getBoundingClientRect();
        const capabilities = document.querySelector('.home-capabilities')!.getBoundingClientRect();
        const overlaps = (first: DOMRect, second: typeof head) =>
          first.left < second.right &&
          first.right > second.left &&
          first.top < second.bottom &&
          first.bottom > second.top;
        return {
          head,
          card: { top: card.top, bottom: card.bottom },
          art: { left: art.left, right: art.right, top: art.top, bottom: art.bottom },
          title: { left: text.left, right: text.right, top: text.top, bottom: text.bottom },
          titleSize: parseFloat(getComputedStyle(title).fontSize),
          titleOverlapsHead: overlaps(text, head),
          headerBottom: document.querySelector('.site-header')!.getBoundingClientRect().bottom,
          capabilitiesTop: capabilities.top,
          materialBlur: getComputedStyle(material).backdropFilter,
          materialOverflow: getComputedStyle(material).overflow,
          cornerRadius: parseFloat(getComputedStyle(material).borderTopLeftRadius),
          scrollWidth: document.documentElement.scrollWidth,
          viewport: innerWidth,
        };
      });
      expect(geometry.card.top - geometry.head.top).toBeGreaterThan(32);
      expect(geometry.head.top).toBeGreaterThanOrEqual(geometry.headerBottom);
      expect(geometry.head.top).toBeGreaterThanOrEqual(geometry.art.top);
      expect(geometry.head.bottom).toBeLessThanOrEqual(geometry.art.bottom);
      expect(geometry.head.left).toBeGreaterThanOrEqual(geometry.art.left);
      expect(geometry.head.right).toBeLessThanOrEqual(geometry.art.right);
      expect(geometry.titleSize).toBeGreaterThanOrEqual(width <= 600 ? 32 : 64);
      expect(geometry.titleOverlapsHead).toBe(false);
      expect(geometry.title.left).toBeGreaterThan(0);
      expect(geometry.title.right).toBeLessThan(geometry.viewport);
      expect(geometry.art.bottom).toBeLessThanOrEqual(geometry.capabilitiesTop);
      expect(geometry.materialBlur).toBe('blur(8px)');
      expect(geometry.materialOverflow).toBe('hidden');
      expect(geometry.cornerRadius).toBe(14);
      expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.viewport);
      await expect(image).toHaveAttribute('src', '/images/home-character.png');
      await expect(page.locator('.home-primary-actions a').first()).toHaveAttribute(
        'href',
        '/register',
      );
      await capture(
        page,
        `hero-tabs-home-${width}-${theme}`,
        `Homepage hero at ${width}px ${theme}; empty synthetic community data; default guest accent, panel blur 8px, opacity 0.55; original character and enlarged wordmark; reduced motion for geometry inspection.`,
      );
    });
  }

  test(`client tabs have no decorative border and retain keyboard focus in ${theme}`, async ({
    page,
  }) => {
    await fixture(page, theme);
    await page.goto('/connect');
    const stable = page.getByRole('tab', { name: 'osu!stable', exact: true });
    const lazer = page.getByRole('tab', { name: 'osu!lazer', exact: true });
    await expect(stable).toHaveAttribute('aria-selected', 'true');
    for (const tab of [stable, lazer]) {
      await expect(tab).toHaveCSS('border-width', '0px');
      await expectNoPaintedShadow(tab);
    }
    await stable.focus();
    await page.keyboard.press('ArrowRight');
    await expect(lazer).toBeFocused();
    await expect(lazer).toHaveAttribute('aria-selected', 'true');
    await expect(stable).toHaveAttribute('aria-selected', 'false');
    await expect(lazer).toHaveCSS('outline-style', 'solid');
    await expect(lazer).toHaveCSS('outline-width', '2px');
    await expect(page.getByRole('tabpanel')).toContainText('Kaleidoscope');
    await capture(
      page,
      `hero-tabs-connect-${theme}`,
      `Connect guide in ${theme}; lazer selected through ArrowRight, retaining visible keyboard focus; both client tabs have no decorative border or shadow.`,
    );
  });

  test(`profile mode has one accent underline in ${theme}`, async ({ page }) => {
    await fixture(page, theme);
    await page.goto(`/players/${playerId}`);
    const rail = page.locator('.profile-mode-rail');
    const active = rail.getByRole('button', { name: 'osu!', exact: true });
    await expect(active).toHaveAttribute('aria-pressed', 'true');
    await expect(active).toHaveCSS('border-bottom-width', '3px');
    await expect(active).toHaveCSS('border-bottom-color', 'rgb(242, 142, 182)');
    await expect(active).toHaveCSS('box-shadow', 'none');
    await expect(rail.getByRole('button', { name: 'taiko', exact: true })).toHaveCSS(
      'border-bottom-color',
      'rgba(0, 0, 0, 0)',
    );
    const taiko = rail.getByRole('button', { name: 'taiko', exact: true });
    await taiko.click();
    await expect(page).toHaveURL(/mode=1/);
    await expect(taiko).toHaveAttribute('aria-pressed', 'true');
    await expect(taiko).toHaveCSS('box-shadow', 'none');
    await expect(active).toHaveAttribute('aria-pressed', 'false');
    await expect(active).toHaveCSS('border-bottom-color', 'rgba(0, 0, 0, 0)');
    await capture(
      page,
      `hero-tabs-profile-${theme}`,
      `Synthetic profile in ${theme}; taiko selected, vanilla score variant; one 3px accent underline per selected group with no inset shadow.`,
    );
  });
}
