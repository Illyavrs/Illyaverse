import { expect, test, type Page } from '@playwright/test';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

type Locale = 'en' | 'zh-TW' | 'ja';
const app = path.resolve(__dirname, '..');
const manifest = JSON.parse(readFileSync(path.join(app, 'public/fonts/provenance.json'), 'utf8'));
const knownAssets = new Map<string, number>(
  manifest.fonts.flatMap((font: { assets: { path: string; bytes: number }[] }) =>
    font.assets.map((asset) => [`/fonts/${asset.path}`, asset.bytes] as const),
  ),
);

async function fixture(page: Page, locale: Locale, theme: 'dark' | 'light') {
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: locale, url: test.info().project.use.baseURL as string },
    ]);
  await page.addInitScript(
    ({ locale, theme }) => {
      localStorage.setItem('theme', theme);
      localStorage.setItem(
        'illya_appearance',
        JSON.stringify({
          mode: theme,
          locale,
          accent_color: '#f28eb6',
          background_url: null,
          background_blur: 0,
          background_dim: 0.76,
          panel_blur: 8,
          panel_opacity: 0.55,
          glass_level: 100,
          border_radius: 14,
        }),
      );
    },
    { locale, theme },
  );
  const unexpected: string[] = [];
  const errors: string[] = [];
  const fontResponses: { path: string; status: number; origin: string }[] = [];
  const fontRequests: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('request', (request) => {
    if (request.resourceType() === 'font') fontRequests.push(request.url());
  });
  page.on('response', (response) => {
    const url = new URL(response.url());
    if (url.pathname.startsWith('/fonts/'))
      fontResponses.push({ path: url.pathname, status: response.status(), origin: url.origin });
  });
  await page.route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url());
    const endpoint = url.pathname.replace('/api/v1', '');
    if (endpoint === '/me')
      return route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } });
    if (endpoint === '/status')
      return route.fulfill({
        json: {
          players_online: 0,
          registered_players: 0,
          ranked_scores: 0,
          beatmap_count: 0,
          compatibility: {},
        },
      });
    if (['/beatmaps', '/leaderboard', '/scores', '/announcements'].includes(endpoint))
      return route.fulfill({ json: { items: [], total: 0, page: 1, page_size: 5 } });
    unexpected.push(endpoint);
    return route.fulfill({ status: 501, json: { error: { code: 'fixture_unexpected' } } });
  });
  return { unexpected, errors, fontResponses, fontRequests };
}

async function platformFonts(page: Page, selector: string) {
  const cdp = await page.context().newCDPSession(page);
  try {
    await cdp.send('DOM.enable');
    await cdp.send('CSS.enable');
    const { root } = await cdp.send('DOM.getDocument');
    const { nodeId } = await cdp.send('DOM.querySelector', { nodeId: root.nodeId, selector });
    expect(nodeId).toBeGreaterThan(0);
    const { fonts } = await cdp.send('CSS.getPlatformFontsForNode', { nodeId });
    return fonts;
  } finally {
    await cdp.detach();
  }
}

const scenes: {
  name: string;
  locale: Locale;
  theme: 'dark' | 'light';
  width: number;
  reflowProxy?: boolean;
}[] = [
  { name: 'en-desktop-dark', locale: 'en', theme: 'dark', width: 1280 },
  { name: 'zh-desktop-dark', locale: 'zh-TW', theme: 'dark', width: 1280 },
  { name: 'zh-phone-light', locale: 'zh-TW', theme: 'light', width: 390 },
  { name: 'ja-desktop-light', locale: 'ja', theme: 'light', width: 1280 },
  { name: 'ja-phone-dark', locale: 'ja', theme: 'dark', width: 390 },
  { name: 'zh-reflow-640-dark', locale: 'zh-TW', theme: 'dark', width: 640, reflowProxy: true },
];

for (const scene of scenes) {
  test(`rounded CJK delivery: ${scene.name}`, async ({ page }) => {
    await page.setViewportSize({ width: scene.width, height: scene.reflowProxy ? 360 : 900 });
    const state = await fixture(page, scene.locale, scene.theme);
    await page.goto('/');
    await expect(page.locator('html')).toHaveAttribute('lang', scene.locale);
    await expect(page.locator('.home-server-status')).toHaveAttribute('aria-busy', 'false');
    await page.evaluate(async () => {
      await document.fonts.ready;
    });
    const textFonts = await platformFonts(page, '.home-description');
    const wordmarkFonts = await platformFonts(page, '.home-wordmark-solid');
    expect(
      wordmarkFonts.some(
        (font) => font.isCustomFont && font.familyName.includes('Nunito') && font.glyphCount > 0,
      ),
    ).toBe(true);
    if (scene.locale !== 'en') {
      const family = scene.locale === 'ja' ? 'Illya Maru' : 'Illya Pearl';
      expect(
        textFonts.some(
          (font) => font.isCustomFont && font.familyName.includes(family) && font.glyphCount > 0,
        ),
      ).toBe(true);
    }
    // Prove decoded glyph use for dynamic, mixed-script content outside current
    // UI translations. The temporary browser-only probe is removed before QA.
    await page.evaluate(() => {
      const probe = document.createElement('div');
      probe.id = 'typography-probe';
      for (const [id, lang, text] of [
        ['type-zh', 'zh-TW', '繁體中文龘𠮷'],
        ['type-ja', 'ja', '日本語ビートマップ'],
        ['type-kana-fallback', 'ja', 'ゕゖ\u3099\u309Aㇰ'],
        ['type-kana-marks', 'ja', '\u3099\u309A'],
        ['type-kana-decomposed', 'ja', 'か\u3099は\u309A'],
        ['type-en', 'en', 'Illyaverse osu! Score 99.82%'],
      ]) {
        const span = document.createElement('span');
        span.id = id;
        span.lang = lang;
        span.textContent = text;
        span.style.fontWeight = '400';
        span.style.display = 'block';
        probe.append(span);
      }
      document.querySelector('.home-intro-content')!.append(probe);
    });
    await page.evaluate(async () => {
      await document.fonts.ready;
    });
    const mixedFonts = {
      zh: await platformFonts(page, '#type-zh'),
      ja: await platformFonts(page, '#type-ja'),
      en: await platformFonts(page, '#type-en'),
      kanaFallback: await platformFonts(page, '#type-kana-fallback'),
      kanaMarks: await platformFonts(page, '#type-kana-marks'),
      kanaDecomposed: await platformFonts(page, '#type-kana-decomposed'),
    };
    expect(
      mixedFonts.zh.some(
        (font) =>
          font.isCustomFont && font.familyName.includes('Illya Pearl') && font.glyphCount >= 6,
      ),
    ).toBe(true);
    expect(
      mixedFonts.ja.some(
        (font) =>
          font.isCustomFont && font.familyName.includes('Illya Maru') && font.glyphCount >= 9,
      ),
    ).toBe(true);
    expect(
      mixedFonts.en.every((font) => font.isCustomFont && font.familyName.includes('Nunito')),
    ).toBe(true);
    expect(
      mixedFonts.kanaFallback.some(
        (font) =>
          font.isCustomFont && font.familyName.includes('Illya Pearl') && font.glyphCount > 0,
      ),
    ).toBe(true);
    expect(
      mixedFonts.kanaMarks.some(
        (font) =>
          font.isCustomFont && font.familyName.includes('Illya Pearl') && font.glyphCount >= 2,
      ),
    ).toBe(true);
    expect(mixedFonts.kanaDecomposed.every((font) => font.isCustomFont)).toBe(true);
    await page.locator('#typography-probe').evaluate((probe) => probe.remove());
    const metrics = await page.evaluate(() => ({
      width: innerWidth,
      document_width: document.documentElement.scrollWidth,
      lang: document.documentElement.lang,
      theme: document.documentElement.className,
      body_font: getComputedStyle(document.body).fontFamily,
      body_size: getComputedStyle(document.body).fontSize,
      body_line_height: getComputedStyle(document.body).lineHeight,
      wordmark_size: getComputedStyle(document.querySelector('.home-wordmark')!).fontSize,
      wordmark_svg_count: document.querySelectorAll('.home-wordmark svg').length,
    }));
    expect(metrics.document_width).toBeLessThanOrEqual(metrics.width);
    expect(metrics.body_font).toContain('Nunito');
    expect(metrics.body_size).toBe('15px');
    expect(metrics.wordmark_svg_count).toBe(0);
    const origin = new URL(test.info().project.use.baseURL as string).origin;
    expect(state.fontResponses.length).toBeGreaterThan(0);
    expect(
      state.fontResponses.every(
        (font) => font.status === 200 && font.origin === origin && knownAssets.has(font.path),
      ),
    ).toBe(true);
    expect(state.fontRequests.every((url) => new URL(url).origin === origin)).toBe(true);
    const requested = [...new Set(state.fontResponses.map((font) => font.path))];
    const payloadBytes = requested.reduce((bytes, asset) => bytes + knownAssets.get(asset)!, 0);
    expect(requested.length).toBeLessThan(35);
    expect(payloadBytes).toBeLessThan(2_000_000);
    expect(state.unexpected).toEqual([]);
    expect(state.errors).toEqual([]);
    if (process.env.ILLYA_CAPTURE_TYPOGRAPHY === '1') {
      const directory = path.resolve(app, '../../.impeccable/review');
      mkdirSync(directory, { recursive: true });
      const filename = `typography-${scene.name}.png`;
      const bytes = await page.screenshot({
        path: path.join(directory, filename),
        fullPage: true,
        animations: 'disabled',
      });
      const sourceFiles = [
        'src/app/layout.tsx',
        'src/app/globals.css',
        'src/styles/cjk-fonts.css',
        'public/fonts/provenance.json',
        'tests/typography.spec.ts',
      ];
      writeFileSync(
        path.join(directory, `typography-${scene.name}.json`),
        JSON.stringify(
          {
            provenance:
              'Synthetic read-only intercepted homepage API fixtures; temporary mixed glyph probe removed before capture. Genuine licensed self-hosted fonts. No live writes or native-client certification.',
            scene,
            filename,
            png_sha256: createHash('sha256').update(bytes).digest('hex'),
            metrics,
            textFonts,
            wordmarkFonts,
            mixedFonts,
            requested,
            payloadBytes,
            source_sha256: Object.fromEntries(
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
      );
    }
  });
}
