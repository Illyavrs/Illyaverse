import { test, expect, type Page } from '@playwright/test';

// All API calls are intercepted. These fixtures never change community accounts.
async function profileFixture(page: Page, guest = false, locale = 'zh-TW') {
  const user = {
    id: 98601,
    username: 'Profile media fixture',
    email: 'profile-media@example.test',
    country: 'TW',
    bio: 'Saved introduction',
    roles: [],
    permissions: [],
    avatar_url: '/media/profile-fixture-avatar.png' as string | null,
    banner_url: '/media/profile-fixture-banner.png' as string | null,
    joined_at: '2026-10-07T00:00:00Z',
  };
  const state = { user, writes: [] as { method: string; body: unknown }[], fail: false };
  const preferences = {
    mode: 'dark',
    accent_color: '#f28eb6',
    background_url: null,
    background_blur: 0,
    background_dim: 0.76,
    panel_blur: 8,
    panel_opacity: 0.55,
    border_radius: 14,
    locale,
  };
  await page.route('**/media/profile-fixture-*.png', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="640" height="200"><rect width="640" height="200" fill="#597b8c"/></svg>',
    }),
  );
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname === '/api/v1/me')
      return route.fulfill(
        guest
          ? {
              status: 401,
              json: { error: { code: 'unauthorized', message: 'Sign in required' } },
            }
          : { json: state.user },
      );
    if (url.pathname === '/api/v1/me/preferences') return route.fulfill({ json: preferences });
    if (url.pathname === '/api/v1/me/media' && request.method() === 'DELETE') {
      const body = request.postDataJSON() as { kind: 'avatar' | 'banner' };
      state.writes.push({ method: request.method(), body });
      if (state.fail)
        return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
      state.user[body.kind === 'avatar' ? 'avatar_url' : 'banner_url'] = null;
      return route.fulfill({ json: { kind: body.kind, url: null } });
    }
    if (url.pathname === '/api/v1/me/profile' && request.method() === 'PATCH') {
      const body = request.postDataJSON() as { bio: string; country: string };
      state.writes.push({ method: request.method(), body });
      Object.assign(state.user, body);
      return route.fulfill({ json: { message: 'Profile updated' } });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'fixture_route_missing' } } });
  });
  return state;
}

async function capture(page: Page, name: string) {
  if (process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW === '1') {
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({
      path: `../../.impeccable/review/workflow-${name}.png`,
      fullPage: true,
      animations: 'disabled',
    });
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
}

test('removing profile images clears only the selected image and preserves unsaved profile fields', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  const state = await profileFixture(page);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/settings?tab=profile');
  await expect(page.getByRole('button', { name: '移除頭像', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '移除橫幅', exact: true })).toBeVisible();
  await page.getByLabel('自我介紹', { exact: true }).fill('Unsaved profile draft');
  await page.getByLabel('國家代碼', { exact: true }).fill('JP');
  await capture(page, 'profile-media-desktop-zhTW');
  await page.getByRole('button', { name: '移除頭像', exact: true }).click();
  await expect(page.getByRole('status')).toHaveText('圖片已移除');
  await expect(page.getByRole('button', { name: '移除頭像', exact: true })).toHaveCount(0);
  await expect(page.locator('.profile-banner-preview')).toBeVisible();
  await expect(page.getByLabel('自我介紹', { exact: true })).toHaveValue('Unsaved profile draft');
  await expect(page.getByLabel('國家代碼', { exact: true })).toHaveValue('JP');
  expect(state.writes).toEqual([{ method: 'DELETE', body: { kind: 'avatar' } }]);
  await page.getByRole('button', { name: '移除橫幅', exact: true }).click();
  await expect(page.locator('.profile-banner-preview')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '移除橫幅', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '儲存變更', exact: true }).click();
  await expect(page.getByRole('status')).toHaveText('已儲存變更');
  expect(state.user.bio).toBe('Unsaved profile draft');
  expect(state.writes.at(-1)).toEqual({
    method: 'PATCH',
    body: { bio: 'Unsaved profile draft', country: 'JP' },
  });
  expect(errors).toEqual([]);
});

test('image-removal failure keeps previews and drafts and offers a working retry on mobile', async ({
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
  const state = await profileFixture(page, false, 'ja');
  state.fail = true;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/settings?tab=profile');
  await page.getByLabel('自己紹介', { exact: true }).fill('Retry draft');
  await page.getByRole('button', { name: 'バナーを削除', exact: true }).click();
  await expect(page.locator('.settings-form').getByRole('alert')).toHaveText(
    '画像を削除できませんでした。しばらくしてから再試行してください。',
  );
  await expect(page.locator('.profile-banner-preview')).toBeVisible();
  await expect(page.getByLabel('自己紹介', { exact: true })).toHaveValue('Retry draft');
  await expect(page.getByRole('button', { name: 'バナーを削除', exact: true })).toBeEnabled();
  await capture(page, 'profile-media-mobile-ja-error');
  state.fail = false;
  await page.getByRole('button', { name: 'バナーを削除', exact: true }).click();
  await expect(page.getByRole('status')).toHaveText('画像を削除しました');
  await expect(page.locator('.settings-form').getByRole('alert')).toHaveCount(0);
  await expect(page.locator('.profile-banner-preview')).toHaveCount(0);
  await expect(page.getByLabel('自己紹介', { exact: true })).toHaveValue('Retry draft');
});

test('guests cannot reach profile media changes', async ({ page }) => {
  const state = await profileFixture(page, true);
  await page.goto('/settings?tab=profile');
  await expect(page.getByRole('heading', { name: '登入後即可繼續', exact: true })).toBeVisible();
  await expect(page.locator('.profile-media-preview')).toHaveCount(0);
  expect(state.writes).toEqual([]);
});
