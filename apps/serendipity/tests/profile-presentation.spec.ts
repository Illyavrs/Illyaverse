import { test, expect, type Page } from '@playwright/test';

// Synthetic intercepted fixtures only; no real community account or artwork is used.
async function fixture(page: Page, locale = 'zh-TW', mode = 'dark') {
  const user = {
    id: 98611,
    username: 'Profile presentation fixture',
    email: 'profile@example.test',
    country: 'TW',
    bio: 'Synthetic profile fixture.',
    avatar_url: null,
    banner_url: '/media/profile-presentation-fixture.svg' as string | null,
    accent_color: '#f28eb6',
    banner_position_x: 50,
    banner_position_y: 50,
    roles: [],
    permissions: [],
    joined_at: '2026-10-07T00:00:00Z',
    pp: 0,
    play_count: 0,
  };
  const preferences = {
    mode,
    locale,
    accent_color: '#f28eb6',
    background_url: null,
    background_blur: 0,
    background_dim: 0.76,
    panel_blur: 8,
    panel_opacity: 0.55,
    border_radius: 14,
  };
  const state = {
    user,
    preferences,
    fail: false,
    uploads: 0,
    writes: [] as Record<string, unknown>[],
  };
  await page.route('**/media/profile-presentation-fixture.svg', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="600" height="900"><rect width="600" height="300" fill="#435664"/><rect y="300" width="600" height="300" fill="#827563"/><rect y="600" width="600" height="300" fill="#586955"/><path d="M300 0v900" stroke="#fff" stroke-width="8"/></svg>',
    }),
  );
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname === '/api/v1/me') return route.fulfill({ json: state.user });
    if (url.pathname === '/api/v1/me/preferences') return route.fulfill({ json: preferences });
    if (url.pathname === '/api/v1/me/relationships')
      return route.fulfill({ json: { friends: [], blocks: [] } });
    if (url.pathname === `/api/v1/users/${user.id}`) {
      const publicUser = Object.fromEntries(
        Object.entries(state.user).filter(([key]) => !['email', 'permissions'].includes(key)),
      );
      return route.fulfill({ json: publicUser });
    }
    if (
      url.pathname === '/api/v1/scores' ||
      url.pathname === `/api/v1/users/${user.id}/scores/best`
    )
      return route.fulfill({ json: { items: [], total: 0, page: 1, page_size: 24 } });
    if (url.pathname === '/api/v1/me/profile' && request.method() === 'PATCH') {
      const body = request.postDataJSON() as Record<string, unknown>;
      state.writes.push(body);
      if (state.fail)
        return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
      Object.assign(state.user, body);
      return route.fulfill({ json: { message: 'Profile updated' } });
    }
    if (url.pathname === '/api/v1/me/media' && request.method() === 'POST') {
      state.uploads++;
      state.user.banner_url = '/media/profile-presentation-fixture.svg';
      return route.fulfill({ json: { kind: 'banner', url: state.user.banner_url } });
    }
    if (url.pathname === '/api/v1/me/media' && request.method() === 'DELETE') {
      state.user.banner_url = null;
      return route.fulfill({ json: { kind: 'banner', url: null } });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'fixture_route_missing' } } });
  });
  return state;
}

async function range(page: Page, id: string, value: number) {
  const slider = page.locator(id).getByRole('slider');
  await slider.press(value <= 50 ? 'Home' : 'End');
  for (let step = 0; step < (value <= 50 ? value : 100 - value); step++)
    await slider.press(value <= 50 ? 'ArrowRight' : 'ArrowLeft');
  await expect(slider).toHaveAttribute('aria-valuenow', String(value));
}

async function capture(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
  // Functional runs retain layout checks without invoking screenshot-only scrolling.
  if (process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW !== '1') return;
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
  await page.screenshot({
    path: `../../.impeccable/review/profile-presentation-${name}.png`,
    fullPage: true,
    animations: 'disabled',
  });
}

test('public profile color and banner position save, reload and stay separate from visitor appearance', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  const state = await fixture(page);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/settings?tab=profile');
  await page.locator('#profile-accent').fill('#767676');
  await range(page, '#banner-position-x', 25);
  await range(page, '#banner-position-y', 80);
  await page.getByLabel('自我介紹', { exact: true }).fill('Public profile presentation draft');
  await expect(page.locator('.profile-banner-preview')).toHaveCSS('background-position', '25% 80%');
  await capture(page, 'settings-desktop-dark-zhTW');
  await page.getByRole('button', { name: '儲存變更', exact: true }).click();
  await expect(page.locator('.form-success')).toHaveText('已儲存變更');
  expect(state.writes).toEqual([
    {
      bio: 'Public profile presentation draft',
      country: 'TW',
      accent_color: '#767676',
      banner_position_x: 25,
      banner_position_y: 80,
    },
  ]);
  await page.reload();
  await expect(page.locator('#profile-accent')).toHaveValue('#767676');
  await expect(page.locator('#banner-position-y').getByRole('slider')).toHaveAttribute(
    'aria-valuenow',
    '80',
  );
  await page.goto(`/players/${state.user.id}`);
  await expect(page.locator('.profile-cover')).toHaveCSS('background-position', '25% 80%');
  const colors = await page.locator('.profile-page').evaluate((element) => ({
    local: getComputedStyle(element).getPropertyValue('--user-accent').trim(),
    global: getComputedStyle(document.documentElement).getPropertyValue('--user-accent').trim(),
    foreground: getComputedStyle(element).getPropertyValue('--primary-foreground').trim(),
  }));
  expect(colors.local).toBe('#767676');
  expect(colors.global).toBe('#f28eb6');
  expect(colors.foreground.toLowerCase()).toMatch(/^(#fff(fff)?|white)$/);
  await capture(page, 'public-desktop-dark-zhTW');
  expect(errors).toEqual([]);
});

test('failed public presentation save retains the complete draft and retries on a light English mobile view', async ({
  page,
  context,
}) => {
  await context.addCookies([
    {
      name: 'illya_locale',
      value: 'en',
      url: process.env.ILLYA_WEB_URL ?? 'http://localhost:3000',
    },
  ]);
  const state = await fixture(page, 'en', 'light');
  state.fail = true;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/settings?tab=profile');
  await page.locator('#profile-accent').fill('#000000');
  await range(page, '#banner-position-x', 0);
  await range(page, '#banner-position-y', 100);
  await page.getByLabel('About you', { exact: true }).fill('Retained profile draft');
  await page.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(page.locator('.settings-form').getByRole('alert')).toBeVisible();
  await expect(page.locator('#profile-accent')).toHaveValue('#000000');
  await expect(page.locator('#banner-position-y').getByRole('slider')).toHaveAttribute(
    'aria-valuenow',
    '100',
  );
  await expect(page.getByLabel('About you', { exact: true })).toHaveValue('Retained profile draft');
  await capture(page, 'settings-mobile-light-en-error');
  state.fail = false;
  await page
    .locator('.settings-form')
    .getByRole('button', { name: 'Try again', exact: true })
    .click();
  await expect(page.locator('.form-success')).toHaveText('Changes saved');
  expect(state.writes).toHaveLength(2);
  expect(state.writes[0]).toEqual(state.writes[1]);
  await page.goto(`/players/${state.user.id}`);
  await expect(page.locator('.profile-cover')).toHaveCSS('background-position', '0% 100%');
  await expect(page.locator('html')).toHaveClass(/light/);
  await capture(page, 'public-mobile-light-en');
});

test('Japanese mobile focal controls preserve presentation drafts through banner removal', async ({
  page,
  context,
}) => {
  await context.addCookies([
    {
      name: 'illya_locale',
      value: 'ja',
      url: process.env.ILLYA_WEB_URL ?? 'http://localhost:3000',
    },
  ]);
  const state = await fixture(page, 'ja');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/settings?tab=profile');
  await range(page, '#banner-position-y', 15);
  await page.locator('#profile-accent').fill('#ffffff');
  await page.getByLabel('自己紹介', { exact: true }).fill('保存前のプロフィール設定');
  await capture(page, 'settings-mobile-dark-ja');
  await page.getByRole('button', { name: 'バナーを削除', exact: true }).click();
  await expect(page.locator('.profile-banner-preview')).toHaveCount(0);
  await expect(page.locator('#profile-accent')).toHaveValue('#ffffff');
  await expect(page.getByLabel('自己紹介', { exact: true })).toHaveValue(
    '保存前のプロフィール設定',
  );
  await page.getByRole('button', { name: '変更を保存', exact: true }).click();
  await expect(page.locator('.form-success')).toContainText('保存しました');
  expect(state.writes[0].banner_position_y).toBe(15);
  await page.goto(`/players/${state.user.id}`);
  await expect(page.locator('.profile-cover')).toHaveCSS('background-image', 'none');
  await capture(page, 'public-mobile-dark-ja-no-banner');
});

test('the same banner file can be uploaded again after removal', async ({ page }) => {
  const state = await fixture(page);
  await page.goto('/settings?tab=profile');
  const bannerInput = page.locator('.profile-media-actions input[type=file]').nth(1);
  const file = {
    name: 'same-file-fixture.png',
    mimeType: 'image/png',
    buffer: Buffer.from('synthetic intercepted upload'),
  };
  await bannerInput.setInputFiles(file);
  await expect.poll(() => state.uploads).toBe(1);
  await expect(bannerInput).toHaveValue('');
  await page.getByRole('button', { name: '移除橫幅', exact: true }).click();
  await expect(page.locator('.profile-banner-preview')).toHaveCount(0);
  await bannerInput.setInputFiles(file);
  await expect.poll(() => state.uploads).toBe(2);
  await expect(page.locator('.profile-banner-preview')).toBeVisible();
});

test('profile publication disables conflicting controls until its saved snapshot finishes', async ({
  page,
}) => {
  const state = await fixture(page);
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => {
    finish = resolve;
  });
  await page.route('**/api/v1/me/profile', async (route) => {
    const submitted = route.request().postDataJSON();
    await pending;
    Object.assign(state.user, submitted);
    await route.fulfill({ json: { message: 'Profile updated' } });
  });
  await page.goto('/settings?tab=profile');
  await page.locator('#profile-accent').fill('#777777');
  await page.getByRole('button', { name: '儲存變更', exact: true }).click();
  await expect(page.locator('.settings-form')).toHaveAttribute('aria-busy', 'true');
  await expect(page.locator('#profile-accent')).toBeDisabled();
  await expect(page.locator('.profile-banner-position')).toHaveAttribute('disabled', '');
  await expect(page.locator('#banner-position-y').getByRole('slider')).toHaveAttribute(
    'data-disabled',
    '',
  );
  await expect(page.locator('#bio')).toBeDisabled();
  finish();
  await expect(page.locator('.form-success')).toHaveText('已儲存變更');
  await expect(page.locator('#profile-accent')).toBeEnabled();
  await page.goto(`/players/${state.user.id}`);
  const foreground = await page
    .locator('.profile-page')
    .evaluate((element) =>
      getComputedStyle(element).getPropertyValue('--primary-foreground').trim(),
    );
  expect(foreground).toBe('#000000');
});
