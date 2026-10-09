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

async function capture(page: Page, name: string, state: string, pausedMotionMs?: number) {
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
    animations: pausedMotionMs === undefined ? 'disabled' : 'allow',
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
        pausedMotionMs,
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
  for (const width of [1440, 1280, 1000, 768, 390, 320]) {
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
        const canvas = document.createElement('canvas');
        canvas.width = image.naturalWidth;
        canvas.height = image.naturalHeight;
        const context = canvas.getContext('2d')!;
        context.drawImage(image, 0, 0);
        // Measure the actual original PNG's opaque crown rather than treating its
        // transparent outer rectangle as a protruding character.
        const pixels = context.getImageData(219, 0, 216, 322).data;
        const opaque = { left: 216, right: 0, top: 322, bottom: 0 };
        for (let y = 0; y < 322; y++) {
          for (let x = 0; x < 216; x++) {
            if (pixels[(y * 216 + x) * 4 + 3] < 240) continue;
            opaque.left = Math.min(opaque.left, x);
            opaque.right = Math.max(opaque.right, x + 1);
            opaque.top = Math.min(opaque.top, y);
            opaque.bottom = Math.max(opaque.bottom, y + 1);
          }
        }
        const head = {
          left: box.left + ((219 + opaque.left) / image.naturalWidth) * box.width,
          right: box.left + ((219 + opaque.right) / image.naturalWidth) * box.width,
          top: box.top + (opaque.top / image.naturalHeight) * box.height,
          bottom: box.top + (opaque.bottom / image.naturalHeight) * box.height,
        };
        const title = document.querySelector('.home-intro-copy h1')!;
        const text = title.getBoundingClientRect();
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
          copyOverlapsArt: [
            ...document.querySelectorAll(
              '.home-intro-copy h1, .home-subtitle, .home-description, .home-primary-actions a',
            ),
          ].some((element) => overlaps(element.getBoundingClientRect(), art)),
          imageWidth: box.width,
          headerBottom: document.querySelector('.site-header')!.getBoundingClientRect().bottom,
          capabilitiesTop: capabilities.top,
          materialBlur: getComputedStyle(material).backdropFilter,
          materialOverflow: getComputedStyle(material).overflow,
          cornerRadius: parseFloat(getComputedStyle(material).borderTopLeftRadius),
          scrollWidth: document.documentElement.scrollWidth,
          viewport: innerWidth,
        };
      });
      expect(geometry.card.top - geometry.head.top).toBeGreaterThan(width <= 600 ? 78 : 94);
      expect(geometry.card.top - geometry.head.top).toBeLessThanOrEqual(112);
      expect(geometry.head.top).toBeGreaterThanOrEqual(geometry.headerBottom);
      expect(geometry.head.top).toBeGreaterThanOrEqual(geometry.art.top);
      expect(geometry.head.bottom).toBeLessThanOrEqual(geometry.art.bottom);
      expect(geometry.head.left).toBeGreaterThanOrEqual(geometry.art.left);
      expect(geometry.head.right).toBeLessThanOrEqual(geometry.art.right);
      expect(geometry.titleSize).toBe(
        width <= 360 ? 32 : width <= 600 ? 36 : width <= 800 ? 56 : width <= 1000 ? 64 : 80,
      );
      expect(geometry.imageWidth).toBe(
        width <= 360 ? 270 : width <= 600 ? 300 : width <= 800 ? 360 : width <= 1000 ? 400 : 480,
      );
      expect(geometry.titleOverlapsHead).toBe(false);
      expect(geometry.copyOverlapsArt).toBe(false);
      expect(geometry.title.left).toBeGreaterThan(0);
      expect(geometry.title.right).toBeLessThan(geometry.viewport);
      expect(geometry.art.bottom).toBeLessThanOrEqual(geometry.capabilitiesTop);
      expect(geometry.materialBlur).toBe('blur(8px)');
      expect(geometry.materialOverflow).toBe('hidden');
      expect(geometry.cornerRadius).toBe(14);
      expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.viewport);
      await expect(image).toHaveAttribute('src', '/images/home-character.png');
      await expect(image).toHaveCSS('animation-name', 'none');
      await expect(page.getByRole('heading', { name: 'Illyaverse', exact: true })).toBeVisible();
      await expect(page.locator('.home-wordmark-outline')).toHaveCSS(
        '-webkit-text-stroke-width',
        '1.5px',
      );
      await expect(page.locator('.home-wordmark-arc').first()).toHaveCSS('animation-name', 'none');
      await expect(page.locator('.home-wordmark-arc').first()).toHaveCSS(
        'stroke-dashoffset',
        '0px',
      );
      await expect(page.locator('.home-primary-actions a').first()).toHaveAttribute(
        'href',
        '/register',
      );
      await capture(
        page,
        `home-wordmark-${width}-${theme}`,
        `Homepage hero at ${width}px ${theme}; empty synthetic community data; default guest accent, panel blur 8px, opacity 0.55; original character at amplified scale, solid/outlined Illyaverse lettering and precise disc arcs; final reduced-motion composition.`,
      );
    });
  }

  for (const width of [1280, 390]) {
    test(`wordmark entrance draws once and rests at ${width}px ${theme}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 1000 });
      await page.emulateMedia({ reducedMotion: 'no-preference' });
      await fixture(page, theme);
      await page.goto('/');
      await page.evaluate(() => document.fonts.ready);
      const title = page.getByRole('heading', { name: 'Illyaverse', exact: true });
      await expect(title).toBeVisible();
      await expect(title).toHaveText('Illyaverse');
      await expect(title).toHaveCSS('opacity', '1');
      await expect(page.locator('.home-wordmark-ink')).toHaveAttribute('aria-hidden', 'true');
      const timeline = await title.evaluate((element) => {
        const animations = element.getAnimations({ subtree: true });
        return animations.map((animation) => {
          animation.pause();
          animation.currentTime = 0;
          const timing = animation.effect!.getTiming();
          return {
            name: (animation as CSSAnimation).animationName,
            duration: timing.duration,
            iterations: timing.iterations,
          };
        });
      });
      expect(timeline).toHaveLength(3);
      expect(
        timeline.every((animation) => animation.duration === 680 && animation.iterations === 1),
      ).toBe(true);
      const arc = page.locator('.home-wordmark-arc').first();
      await expect(arc).toHaveCSS('stroke-dashoffset', '1px');
      await expect(page.locator('.home-character-image')).toHaveCSS('animation-name', 'none');
      await title.evaluate((element) => {
        for (const animation of element.getAnimations({ subtree: true }))
          animation.currentTime = 280;
      });
      const moving = await page.locator('.home-wordmark-ink').evaluate((element) => ({
        ink: Number(getComputedStyle(element).opacity),
        arc: parseFloat(
          getComputedStyle(document.querySelector('.home-wordmark-arc')!).strokeDashoffset,
        ),
      }));
      expect(moving.arc).toBeGreaterThan(0);
      expect(moving.arc).toBeLessThan(1);
      expect(moving.ink).toBeGreaterThan(0.5);
      const layers = await page.locator('.home-wordmark-verse').evaluate((element) => {
        const outline = element.querySelector('.home-wordmark-outline')!;
        const ink = element.querySelector('.home-wordmark-ink')!;
        const bounds = (node: Element) => {
          const box = node.getBoundingClientRect();
          return { x: box.x, y: box.y, width: box.width, height: box.height };
        };
        return {
          outline: bounds(outline),
          ink: bounds(ink),
          outlineLineHeight: getComputedStyle(outline).lineHeight,
          inkLineHeight: getComputedStyle(ink).lineHeight,
          outlineFont: getComputedStyle(outline).font,
          inkFont: getComputedStyle(ink, '::before').font,
        };
      });
      // Measure the actually painted ink layer's grid box and font: both glyph
      // tracks must share a baseline at desktop and phone sizes, including mid-reveal.
      expect(layers.ink).toEqual(layers.outline);
      expect(layers.inkLineHeight).toBe(layers.outlineLineHeight);
      expect(layers.inkFont).toBe(layers.outlineFont);
      await capture(
        page,
        `home-wordmark-${width}-${theme}-motion-280ms`,
        `Synthetic guest hero; actual single CSS wordmark entrance replay paused at 280ms after fonts settle, so line drawing and solid ink reveal are visible; the original character stays still.`,
        280,
      );
      await title.evaluate((element) => {
        for (const animation of element.getAnimations({ subtree: true })) {
          animation.currentTime = 680;
          animation.finish();
        }
      });
      await expect(arc).toHaveCSS('stroke-dashoffset', '0px');
      expect(
        await page
          .locator('.home-wordmark-ink')
          .evaluate((element) => Number(getComputedStyle(element).opacity)),
      ).toBe(0);
      expect(
        await title.evaluate((element) =>
          element
            .getAnimations({ subtree: true })
            .every((animation) => animation.playState === 'finished'),
        ),
      ).toBe(true);
      await page.emulateMedia({ reducedMotion: 'reduce' });
      await expect(arc).toHaveCSS('animation-name', 'none');
      await expect(arc).toHaveCSS('stroke-dashoffset', '0px');
      await expect(title).toHaveAccessibleName('Illyaverse');
      expect(
        await page.locator('.home-wordmark-ink').evaluate((element) => ({
          animation: getComputedStyle(element).animationName,
          ink: Number(getComputedStyle(element).opacity),
        })),
      ).toEqual({ animation: 'none', ink: 0 });
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
      `home-wordmark-regression-connect-${theme}`,
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
      `home-wordmark-regression-profile-${theme}`,
      `Synthetic profile in ${theme}; taiko selected, vanilla score variant; one 3px accent underline per selected group with no inset shadow.`,
    );
  });
}
