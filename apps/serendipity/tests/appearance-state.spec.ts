import { selectField } from './helpers/field-select';
import { test, expect, type Page } from '@playwright/test';

// Synthetic intercepted settings; never edits a real player's account or media.
async function fixture(page: Page, locale = 'zh-TW', mode = 'dark', guest = false) {
  const preferences = {
    locale, mode, accent_color: '#f28eb6', background_url: null as string | null,
    background_blur: 0, background_dim: 0.76, panel_blur: 8, panel_opacity: 0.55,
    border_radius: 14, glass_level: 100,
  };
  const user = {
    id: 98631, username: 'Appearance state fixture', email: 'appearance@example.test',
    country: 'TW', bio: '', avatar_url: null, banner_url: null, accent_color: '#f28eb6',
    banner_position_x: 50, banner_position_y: 50, roles: [], permissions: [],
    joined_at: '2026-10-07T00:00:00Z', pp: 0, play_count: 0,
  };
  const state = {
    preferences, writes: [] as Record<string, unknown>[], uploads: 0,
    failSave: false, failUpload: false, legacyPreferences: false,
    gate: null as Promise<void> | null,
  };
  await page.context().addCookies([{ name: 'illya_locale', value: locale, url: test.info().project.use.baseURL! }]);
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const pathname = new URL(request.url()).pathname;
    if (pathname === '/api/v1/me') return guest
      ? route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } })
      : route.fulfill({ json: user });
    if (pathname === '/api/v1/me/preferences' && request.method() === 'GET') {
      const legacy = { ...preferences } as Record<string, unknown>;
      delete legacy.glass_level;
      return route.fulfill({ json: state.legacyPreferences ? legacy : preferences });
    }
    if (pathname === '/api/v1/me/preferences' && request.method() === 'PATCH') {
      const body = request.postDataJSON() as Record<string, unknown>;
      state.writes.push(body);
      if ('accent_color' in body && state.gate) await state.gate;
      if ('accent_color' in body && state.failSave)
        return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
      Object.assign(preferences, body);
      return route.fulfill({ json: preferences });
    }
    if (pathname === '/api/v1/me/media' && request.method() === 'POST') {
      state.uploads++;
      if (state.gate) await state.gate;
      if (state.failUpload)
        return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
      preferences.background_url = '/media/appearance-state-fixture.png';
      return route.fulfill({ json: { url: preferences.background_url } });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'fixture_route_missing' } } });
  });
  await page.route('**/media/appearance-state-fixture.png', (route) => route.fulfill({
    contentType: 'image/png',
    body: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=', 'base64'),
  }));
  return state;
}

async function capture(page: Page, name: string, fullPage = true) {
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => {
    document.documentElement.style.setProperty('scroll-behavior', 'auto', 'important');
    document.body.style.setProperty('scroll-behavior', 'auto', 'important');
    (document.activeElement as HTMLElement | null)?.blur();
    window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
  });
  await page.mouse.move(0, 0);
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  if (process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW === '1') await page.screenshot({
    path: `../../.impeccable/review/appearance-state-${name}.png`, fullPage, animations: 'disabled',
  });
}

test('delayed appearance publication locks shared controls and keeps language writes independent', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  const state = await fixture(page);
  await page.goto('/settings');
  const controls = page.locator('.appearance-controls:not(.compact)');
  await page.locator('#accent').fill('#767676');
  let release!: () => void;
  state.gate = new Promise<void>((resolve) => { release = resolve; });
  await controls.getByRole('button', { name: '儲存變更', exact: true }).click();
  await expect.poll(() => state.writes.length).toBe(1);
  await expect(page.locator('#accent')).toBeDisabled();
  await expect(controls.locator('.segmented button').first()).toBeDisabled();
  await expect(controls.locator('.swatches button').first()).toBeDisabled();
  await expect(page.locator('#panel_blur')).toHaveAttribute('data-disabled', '');
  await expect(controls.getByRole('button', { name: '重設外觀', exact: true })).toBeDisabled();
  await expect(page.locator('.theme-toggle')).toBeDisabled();
  await capture(page, 'desktop-dark-zhTW-pending');
  await page.locator('.header-tools button').filter({ has: page.locator('svg.lucide-palette') }).click();
  const quick = page.locator('.appearance-controls.compact');
  await expect(page.locator('#quick-accent')).toBeDisabled();
  await expect(quick.locator('#quick-panel_blur')).toHaveAttribute('data-disabled', '');
  await capture(page, 'desktop-dark-zhTW-pending-sheet', false);
  await page.keyboard.press('Escape');
  // Language selection has its own PATCH; the pending appearance snapshot omits locale.
  await selectField(page.locator('.language-select'), 'en');
  await expect.poll(() => state.writes.some((write) => write.locale === 'en')).toBe(true);
  expect(state.writes[0]).not.toHaveProperty('locale');
  release();
  await expect(page.locator('#accent')).toBeEnabled();
  await expect(page.locator('.appearance-controls:not(.compact) .form-success')).toBeVisible();
  expect(state.preferences.locale).toBe('en');
  expect(state.preferences.accent_color).toBe('#767676');
  await page.reload();
  await expect(page.locator('.language-select')).toHaveAttribute('data-value', 'en');
  await expect(page.locator('#accent')).toHaveValue('#767676');
});

test('appearance reset preserves English, failure keeps draft and retry saves only appearance', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await fixture(page, 'en', 'light');
  await page.goto('/settings');
  const controls = page.locator('.appearance-controls:not(.compact)');
  await controls.getByRole('button', { name: 'Reset appearance', exact: true }).click();
  await expect(page.locator('.language-select')).toHaveAttribute('data-value', 'en');
  await page.locator('#accent').fill('#777777');
  await controls.getByRole('button', { name: 'Light', exact: true }).click();
  await expect(controls.locator('.form-success')).toHaveCount(0);
  state.failSave = true;
  await controls.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(controls.getByRole('alert')).toBeVisible();
  await expect(page.locator('#accent')).toHaveValue('#777777');
  await expect(page.locator('.language-select')).toHaveAttribute('data-value', 'en');
  await capture(page, 'mobile-light-en-save-error');
  state.failSave = false;
  await controls.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(controls.locator('.form-success')).toBeVisible();
  expect(state.writes).toHaveLength(2);
  for (const write of state.writes) expect(write).not.toHaveProperty('locale');
  expect(state.preferences.locale).toBe('en');
  await page.reload();
  await expect(page.locator('.language-select')).toHaveAttribute('data-value', 'en');
  await expect(page.locator('#accent')).toHaveValue('#777777');
});

test('guest reset with no appearance cache preserves Japanese cookie and reload', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await fixture(page, 'ja', 'dark', true);
  await page.goto('/settings');
  const controls = page.locator('.appearance-controls:not(.compact)');
  await expect(page.locator('#accent')).toBeVisible();
  await page.locator('#accent').fill('#000000');
  await controls.getByRole('button', { name: '外観をリセット', exact: true }).click();
  await expect(page.locator('.language-select')).toHaveAttribute('data-value', 'ja');
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('illya_appearance')!).locale)).toBe('ja');
  await capture(page, 'mobile-dark-ja-guest-reset');
  await page.reload();
  await expect(page.locator('.language-select')).toHaveAttribute('data-value', 'ja');
  await expect(page.locator('#accent')).toHaveValue('#f28eb6');
  expect(state.writes).toHaveLength(0);
});

test('accent action text meets contrast and black or white retain visible keyboard focus', async ({ page }) => {
  await fixture(page, 'en');
  await page.goto('/settings');
  await page.locator('#accent').fill('#767676');
  await expect.poll(() => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--primary-contrast').trim())).toBe('#ffffff');
  await page.locator('#accent').fill('#777777');
  await expect.poll(() => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--primary-contrast').trim())).toBe('#000000');
  const save = page.locator('.appearance-controls:not(.compact)').getByRole('button', { name: 'Save changes', exact: true });
  for (const accent of ['#767676', '#777777', '#000000', '#ffffff']) {
    await page.locator('#accent').fill(accent);
    await expect.poll(async () => {
      const colors = await save.evaluate((element) => {
        const style = getComputedStyle(element);
        return [style.backgroundColor, style.color];
      });
      const luminances = colors.map((color) => {
        const channels = color.match(/[\d.]+/g)!.slice(0, 3).map(Number)
          .map((channel) => channel / 255)
          .map((channel) => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
        return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
      });
      return (Math.max(...luminances) + 0.05) / (Math.min(...luminances) + 0.05);
    }).toBeGreaterThanOrEqual(4.5);
  }
  for (const [accent, mode] of [['#000000', 'Dark'], ['#ffffff', 'Light']]) {
    await page.locator('.appearance-controls:not(.compact)').getByRole('button', { name: mode, exact: true }).click();
    await page.locator('#accent').fill(accent);
    await page.locator('#accent').focus();
    await page.keyboard.press('Tab');
    const focus = await page.evaluate(() => {
      const element = document.activeElement!;
      return { outline: getComputedStyle(element).outlineColor, width: getComputedStyle(element).outlineWidth };
    });
    expect(focus.width).toBe('2px');
    expect(focus.outline).not.toBe(accent === '#000000' ? 'rgb(0, 0, 0)' : 'rgb(255, 255, 255)');
  }
});

test('failed background upload retains appearance draft and retries the same file', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await fixture(page, 'ja');
  await page.goto('/settings');
  const controls = page.locator('.appearance-controls:not(.compact)');
  await page.locator('#accent').fill('#7dd8c5');
  state.failUpload = true;
  const file = {
    name: 'appearance-fixture.png', mimeType: 'image/png',
    buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=', 'base64'),
  };
  await controls.locator('input[type=file]').setInputFiles(file);
  await expect(controls.getByRole('alert')).toBeVisible();
  await expect(page.locator('#accent')).toHaveValue('#7dd8c5');
  await expect(controls.locator('.form-success')).toHaveCount(0);
  await capture(page, 'mobile-dark-ja-upload-error');
  state.failUpload = false;
  await controls.getByRole('button', { name: '再試行', exact: true }).click();
  await expect.poll(() => state.uploads).toBe(2);
  await expect(controls.locator('.form-success')).toBeVisible();
  await expect(page.locator('#accent')).toHaveValue('#7dd8c5');
  await expect(controls.locator('.background-preview')).toHaveCSS('background-image', /appearance-state-fixture\.png/);
  await controls.locator('input[type=file]').setInputFiles(file);
  await expect.poll(() => state.uploads).toBe(3);
});

test('glass coverage previews solid and glass roles without changing blur or opacity', async ({ page }) => {
  await fixture(page, 'en');
  await page.goto('/settings');
  const slider = page.locator('#glass_level').getByRole('slider');
  await expect(slider).toHaveAttribute('aria-valuenow', '100');
  await page.evaluate(() => {
    for (const material of ['identity', 'section', 'content']) {
      const surface = document.createElement('div');
      surface.className = 'glass';
      surface.dataset.material = material;
      surface.dataset.testid = `material-${material}`;
      document.body.append(surface);
    }
  });
  const roles = ['identity', 'section', 'content'] as const;
  for (const mode of ['Dark', 'Light']) {
    await page.locator('.appearance-controls:not(.compact)').getByRole('button', { name: mode, exact: true }).click();
    for (const [level, tier, activeRoles] of [
      [0, 'navigation', 0], [25, 'identity', 1], [50, 'section', 2], [75, 'content', 3], [100, 'content', 3],
    ] as const) {
      await slider.focus();
      await page.keyboard.press('Home');
      for (let value = 0; value < Math.floor(level / 10); value++) await page.keyboard.press('PageUp');
      for (let value = 0; value < level % 10; value++) await page.keyboard.press('ArrowRight');
      await expect(page.locator('html')).toHaveAttribute('data-glass-tier', tier);
      await expect(slider).toHaveAttribute('aria-valuenow', String(level));
      for (const [index, material] of roles.entries()) {
        const surface = page.getByTestId(`material-${material}`);
        await expect(surface).toHaveCSS('backdrop-filter', index < activeRoles ? 'blur(8px)' : 'blur(0px)');
        const color = await surface.evaluate((element) => getComputedStyle(element).backgroundColor);
        if (index < activeRoles) expect(color).toMatch(/0\.55\)$/);
        else expect(color).toBe(mode === 'Dark' ? 'rgb(24, 24, 27)' : 'rgb(255, 255, 255)');
      }
      await expect(page.locator('.site-header')).toHaveCSS('backdrop-filter', 'blur(8px)');
      const values = await page.evaluate(() => {
        const style = getComputedStyle(document.documentElement);
        return [style.getPropertyValue('--panel-blur').trim(), style.getPropertyValue('--panel-opacity').trim()];
      });
      expect(values).toEqual(['8px', '0.55']);
    }
  }
  await slider.focus();
  await page.keyboard.press('Home');
  await page.locator('.header-tools button').filter({ has: page.locator('svg.lucide-palette') }).click();
  const sheet = page.locator('.appearance-sheet');
  await expect(sheet).toHaveCSS('backdrop-filter', 'blur(8px)');
  await expect(page.locator('#quick-glass_level').getByRole('slider')).toHaveAttribute('aria-valuenow', '0');
});

test('glass coverage survives guest reload and upgrades an older local preference', async ({ page }) => {
  await fixture(page, 'en', 'dark', true);
  await page.addInitScript(() => {
    if (!localStorage.getItem('illya_appearance')) localStorage.setItem('illya_appearance', JSON.stringify({
      mode: 'light', accent_color: '#7dd8c5', panel_blur: 17, panel_opacity: 0.42, border_radius: 20,
    }));
  });
  await page.goto('/settings');
  const slider = page.locator('#glass_level').getByRole('slider');
  await expect(slider).toHaveAttribute('aria-valuenow', '100');
  await expect(page.locator('#panel_blur').getByRole('slider')).toHaveAttribute('aria-valuenow', '17');
  await expect(page.locator('#panel_opacity').getByRole('slider')).toHaveAttribute('aria-valuenow', '42');
  await slider.focus();
  await page.keyboard.press('Home');
  await page.reload();
  await expect(page.locator('#glass_level').getByRole('slider')).toHaveAttribute('aria-valuenow', '0');
  await expect(page.locator('html')).toHaveAttribute('data-glass-tier', 'navigation');
  await expect(page.locator('#panel_blur').getByRole('slider')).toHaveAttribute('aria-valuenow', '17');
  await expect(page.locator('#panel_opacity').getByRole('slider')).toHaveAttribute('aria-valuenow', '42');
});

test('old account preferences gain coverage default and save failure preserves its retry draft', async ({ page }) => {
  const state = await fixture(page, 'en');
  state.legacyPreferences = true;
  await page.goto('/settings');
  const controls = page.locator('.appearance-controls:not(.compact)');
  const slider = page.locator('#glass_level').getByRole('slider');
  await expect(slider).toHaveAttribute('aria-valuenow', '100');
  await slider.focus();
  await page.keyboard.press('Home');
  state.failSave = true;
  await controls.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(controls.getByRole('alert')).toBeVisible();
  await expect(slider).toHaveAttribute('aria-valuenow', '0');
  expect(state.writes[0]).toMatchObject({ glass_level: 0, panel_blur: 8, panel_opacity: 0.55 });
  state.failSave = false;
  state.legacyPreferences = false;
  await controls.getByRole('button', { name: 'Try again', exact: true }).click();
  await expect(controls.locator('.form-success')).toBeVisible();
  await page.reload();
  await expect(page.locator('#glass_level').getByRole('slider')).toHaveAttribute('aria-valuenow', '0');
  expect(state.writes).toHaveLength(2);
});

test('language Select supports keyboard selection, Escape focus and bounded menus in both themes', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 844 });
  const state = await fixture(page, 'en');
  await page.goto('/settings');
  const language = page.locator('.language-select');
  const controls = page.locator('.appearance-controls:not(.compact)');
  for (const theme of ['Dark', 'Light']) {
    await controls.getByRole('button', { name: theme, exact: true }).click();
    await language.focus();
    await language.press('Enter');
    const listbox = page.getByRole('listbox');
    await expect(listbox).toBeVisible();
    await expect(listbox.getByRole('option')).toHaveCount(3);
    await expect(listbox.getByRole('option', { name: 'EN', exact: true })).toHaveAttribute('aria-selected', 'true');
    await expect(listbox).toHaveCSS('backdrop-filter', 'blur(8px)');
    const bounds = await listbox.boundingBox();
    expect(bounds?.x).toBeGreaterThanOrEqual(0);
    expect((bounds?.x ?? 0) + (bounds?.width ?? 0)).toBeLessThanOrEqual(320);
    await capture(page, `mobile-${theme.toLowerCase()}-en-language-popup`, false);
    await page.keyboard.press('Escape');
    await expect(listbox).toHaveCount(0);
    await expect(language).toBeFocused();
    await expect(language).toHaveAttribute('data-value', 'en');
  }
  await language.press('Enter');
  await expect(page.getByRole('listbox')).toBeVisible();
  await expect(page.getByRole('option', { name: 'EN', exact: true })).toBeFocused();
  await page.keyboard.press('End');
  await expect(page.getByRole('option', { name: '日本語', exact: true })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(language).toHaveAttribute('data-value', 'ja');
  await expect.poll(() => state.writes.some((write) => write.locale === 'ja')).toBe(true);
  await language.focus();
  await language.press('Enter');
  await expect(page.getByRole('listbox')).toBeVisible();
  await expect(page.getByRole('option', { name: '日本語', exact: true })).toBeFocused();
  await page.keyboard.press('Home');
  await expect(page.getByRole('option', { name: '繁中', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('option', { name: 'EN', exact: true })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(language).toHaveAttribute('data-value', 'en');
  await expect(language).toBeFocused();
});
