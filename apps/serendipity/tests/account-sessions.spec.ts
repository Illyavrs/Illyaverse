import { test, expect, type Page } from '@playwright/test';

type Session = {
  id: string;
  client: 'web' | 'stable' | 'lazer';
  current: boolean;
  created_at: string;
  expires_at: string;
  last_seen_at: string | null;
};

const clients = ['web', 'web', 'stable', 'lazer'] as const;
const sampleSessions: Session[] = clients.map((client, index) => ({
  id: `00000000-0000-4000-8000-00000000000${index + 1}`,
  client,
  current: index === 0,
  created_at: '2026-10-09T01:00:00Z',
  expires_at: '2026-11-08T01:00:00Z',
  last_seen_at: client === 'lazer' ? '2026-10-09T05:30:00Z' : null,
}));

// Every API request is intercepted; these tests never revoke real logins.
async function fixture(page: Page, locale = 'zh-TW') {
  await page.context().addCookies([
    {
      name: 'illya_locale',
      value: locale,
      url: new URL(process.env.ILLYA_WEB_URL ?? 'http://localhost:3000').origin,
    },
  ]);
  const preferences = {
    mode: 'dark',
    accent_color: '#f28eb6',
    background_url: null,
    background_blur: 0,
    background_dim: 0.76,
    panel_blur: 8,
    panel_opacity: 0.55,
    glass_level: 100,
    border_radius: 14,
    locale,
  };
  const state = {
    sessions: structuredClone(sampleSessions),
    authenticated: true,
    hasMore: false,
    listStatus: 200,
    writeStatus: 200,
    failRefreshAfterWrite: false,
    holdList: null as null | Promise<void>,
    holdWrite: null as null | Promise<void>,
    writes: [] as string[],
    apiMutations: [] as string[],
    gets: 0,
    preferences,
  };
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (request.method() !== 'GET') state.apiMutations.push(`${request.method()} ${path}`);
    if (path === '/api/v1/me')
      return route.fulfill(
        state.authenticated
          ? {
              json: {
                id: 98823,
                username: 'Session fixture',
                email: 'sessions@example.test',
                email_verified: true,
                country: 'TW',
                bio: '',
                joined_at: '2026-10-01T00:00:00Z',
                permissions: [],
                roles: [],
                avatar_url: null,
                banner_url: null,
              },
            }
          : { status: 401, json: { error: { code: 'unauthorized' } } },
      );
    if (path === '/api/v1/me/preferences') return route.fulfill({ json: preferences });
    if (path === '/api/v1/me/sessions' && request.method() === 'GET') {
      state.gets++;
      if (state.holdList) await state.holdList;
      return route.fulfill(
        state.listStatus === 200
          ? { json: { sessions: state.sessions, has_more: state.hasMore } }
          : { status: state.listStatus, json: { error: { code: 'fixture_error' } } },
      );
    }
    if (path.startsWith('/api/v1/me/sessions/')) {
      state.writes.push(`${request.method()} ${path}`);
      if (state.holdWrite) await state.holdWrite;
      if (state.writeStatus !== 200)
        return route.fulfill({
          status: state.writeStatus,
          json: { error: { code: 'fixture_error' } },
        });
      if (state.failRefreshAfterWrite) state.listStatus = 503;
      if (path.endsWith('/revoke-others')) {
        expect(request.method()).toBe('POST');
        state.sessions = state.sessions.filter((session) => session.current);
        state.hasMore = false;
        return route.fulfill({ json: { ok: true } });
      }
      expect(request.method()).toBe('DELETE');
      const selected = state.sessions.find((session) =>
        path.endsWith(`/${session.client}/${session.id}`),
      );
      if (!selected) return route.fulfill({ status: 404, json: { error: { code: 'not_found' } } });
      state.sessions = state.sessions.filter((session) => session !== selected);
      if (selected.current) state.authenticated = false;
      return route.fulfill({ json: { ok: true, current_revoked: selected.current } });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'unexpected_fixture_route' } } });
  });
  return state;
}

async function capture(page: Page, name: string) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  if (process.env.ILLYA_CAPTURE_ACCOUNT_SESSIONS === '1') {
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({
      path: `../../.impeccable/review/account-sessions-${name}.png`,
      fullPage: true,
      animations: 'disabled',
    });
  }
}

test('account sessions show actual client types and timestamps without invented device details', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  const state = await fixture(page);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/settings?tab=account');
  const section = page.getByRole('region', { name: '登入工作階段', exact: true });
  await expect(section.getByRole('listitem')).toHaveCount(4);
  await expect(section.getByText('目前登入', { exact: true })).toHaveCount(1);
  await expect(section.getByText('最後活動', { exact: true })).toHaveCount(1);
  await expect(section.locator('time')).toHaveCount(9);
  await expect(section.locator('[data-session-client="stable"]')).toHaveText(/osu!stable/);
  await expect(section.locator('[data-session-client="lazer"]')).toHaveText(/osu!lazer/);
  await expect(section).toContainText('登入有效不代表玩家目前在線');
  expect(state.writes).toEqual([]);
  await capture(page, 'desktop-zh-TW-dark-list');
  const beforeDialog = await section.boundingBox();
  await section.getByRole('button', { name: '登出其他登入', exact: true }).click();
  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toBeVisible();
  const duringDialog = await section.boundingBox();
  expect(Math.abs(beforeDialog!.x - duringDialog!.x)).toBeLessThan(1);
  expect(Math.abs(beforeDialog!.width - duringDialog!.width)).toBeLessThan(1);
  expect(await dialog.evaluate((node) => getComputedStyle(node).backdropFilter)).toBe('blur(8px)');
  await capture(page, 'desktop-zh-TW-dark-confirm');
  await dialog.getByRole('button', { name: '取消', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  const afterDialog = await section.boundingBox();
  expect(Math.abs(beforeDialog!.x - afterDialog!.x)).toBeLessThan(1);
  state.preferences.mode = 'light';
  state.preferences.glass_level = 0;
  state.preferences.panel_blur = 16;
  await page.reload();
  await expect(page.locator('html')).toHaveClass(/light/);
  await expect(section.getByRole('listitem')).toHaveCount(4);
  await capture(page, 'desktop-zh-TW-light-navigation-list');
  await section.getByRole('button', { name: '登出其他登入', exact: true }).click();
  await expect(dialog).toBeVisible();
  expect(await dialog.evaluate((node) => getComputedStyle(node).backdropFilter)).toBe('blur(16px)');
  await capture(page, 'desktop-zh-TW-light-navigation-confirm');
  await dialog.getByRole('button', { name: '取消', exact: true }).click();
  expect(errors).toEqual([]);
});

test('individual sign-out prevents duplicate writes and preserves the password form draft', async ({
  page,
}) => {
  const state = await fixture(page);
  let release!: () => void;
  state.holdWrite = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.goto('/settings?tab=account');
  await page.getByLabel('目前密碼', { exact: true }).fill('Unsaved-fixture-password');
  const stable = page.locator('[data-session-client="stable"]');
  const action = stable.getByRole('button');
  await action.click();
  await expect(action).toBeDisabled();
  await action.dispatchEvent('click');
  await expect(page.locator('[data-session-client="lazer"]').getByRole('button')).toBeEnabled();
  await expect(page.getByLabel('目前密碼', { exact: true })).toBeEnabled();
  await expect.poll(() => state.writes.length).toBe(1);
  release();
  await expect(stable).toHaveCount(0);
  await expect(page.getByText('已登出所選工作階段。', { exact: true })).toBeVisible();
  await expect(page.getByLabel('目前密碼', { exact: true })).toHaveValue(
    'Unsaved-fixture-password',
  );
  expect(state.writes).toEqual([
    'DELETE /api/v1/me/sessions/stable/00000000-0000-4000-8000-000000000003',
  ]);
});

test('sign-out failure keeps the session and gives an actionable retry after refresh', async ({
  page,
}) => {
  const state = await fixture(page);
  state.writeStatus = 429;
  await page.goto('/settings?tab=account');
  const row = page.locator('[data-session-client="lazer"]');
  await row.getByRole('button').click();
  await expect(page.getByRole('alert')).toContainText('請求過於頻繁');
  await expect(row).toBeVisible();
  await expect(row.getByRole('button')).toBeEnabled();
  state.writeStatus = 200;
  await row.getByRole('button').click();
  await expect(row).toHaveCount(0);
  expect(state.writes).toHaveLength(2);
});

test('a saved sign-out stays removed if the subsequent list refresh fails', async ({ page }) => {
  const state = await fixture(page);
  state.failRefreshAfterWrite = true;
  await page.goto('/settings?tab=account');
  const row = page.locator('[data-session-client="stable"]');
  await row.getByRole('button').click();
  await expect(row).toHaveCount(0);
  await expect(page.getByRole('alert')).toContainText('已完成的登出仍有效');
  state.listStatus = 200;
  await page.getByRole('button', { name: '重新整理列表', exact: true }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(
    page.getByRole('list', { name: '你的有效登入工作階段' }).getByRole('listitem'),
  ).toHaveCount(3);
});

test('revoke other sessions confirms the full bounded scope and restores keyboard focus', async ({
  page,
}) => {
  const state = await fixture(page, 'en');
  state.hasMore = true;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/settings?tab=account');
  const action = page.getByRole('button', { name: 'Sign out other sessions', exact: true });
  await expect(page.getByText(/Up to 100 sessions are listed/)).toBeVisible();
  await action.focus();
  await page.keyboard.press('Enter');
  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toContainText('all other website, stable and lazer sessions');
  await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeFocused();
  expect(state.writes).toHaveLength(0);
  await capture(page, 'mobile-en-confirm');
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(action).toBeFocused();
  await action.click();
  await page
    .getByRole('alertdialog')
    .getByRole('button', { name: 'Sign out', exact: true })
    .click();
  await expect(page.getByRole('alertdialog')).toHaveCount(0);
  await expect(
    page.getByRole('list', { name: 'Your valid login sessions' }).getByRole('listitem'),
  ).toHaveCount(1);
  await expect(page.getByText(/This website login is still valid/)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Refresh list', exact: true })).toBeFocused();
  await expect(page.getByRole('link', { name: 'Session fixture', exact: true })).toBeVisible();
  expect(state.writes).toEqual(['POST /api/v1/me/sessions/revoke-others']);
  await capture(page, 'mobile-en-others-revoked');
});

test('current website session asks before sign-out then clears authenticated UI and redirects', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/settings?tab=account');
  await page.getByRole('button', { name: '登出目前的網站工作階段', exact: true }).click();
  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toContainText('尚未儲存的帳號設定不會被儲存');
  expect(state.writes).toEqual([]);
  await dialog.getByRole('button', { name: '登出', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.locator('[data-session-client]')).toHaveCount(0);
  await expect(page.getByRole('link', { name: 'Session fixture', exact: true })).toHaveCount(0);
  expect(state.authenticated).toBe(false);
  expect(state.writes).toEqual([
    'DELETE /api/v1/me/sessions/web/00000000-0000-4000-8000-000000000001',
  ]);
});

test('initial loading failure retries and Japanese empty state remains readable on mobile', async ({
  page,
}) => {
  const state = await fixture(page, 'ja');
  state.listStatus = 503;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/settings?tab=account');
  await expect(page.getByRole('alert')).toContainText('一覧を更新して再試行');
  state.listStatus = 200;
  state.sessions = [];
  await page.getByRole('button', { name: '一覧を更新', exact: true }).click();
  await expect(
    page.getByText('有効なログインセッションはありません。', { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole('button', { name: '他のセッションをログアウト', exact: true }),
  ).toBeDisabled();
  await capture(page, 'mobile-ja-empty');
});

test('an unauthorized session list clears the cached account and replaces the protected history entry', async ({
  page,
}) => {
  const state = await fixture(page);
  state.listStatus = 401;
  let release!: () => void;
  state.holdList = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.goto('/login?history-marker=sessions');
  await expect(page.getByRole('link', { name: 'Session fixture', exact: true })).toBeVisible();
  await page.goto('/settings?tab=account');
  await expect.poll(() => state.gets).toBe(1);
  await expect(page.getByRole('link', { name: 'Session fixture', exact: true })).toBeVisible();
  const historyLength = await page.evaluate(() => history.length);
  release();
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.locator('[data-session-client]')).toHaveCount(0);
  await expect(page.getByRole('link', { name: 'Session fixture', exact: true })).toHaveCount(0);
  // /me still returns the fixture user: the UI must clear its cached identity
  // itself, without submitting another sign-out request after the 401.
  expect(state.authenticated).toBe(true);
  expect(state.apiMutations).toEqual([]);
  expect(await page.evaluate(() => history.length)).toBe(historyLength);
  await page.goBack();
  await expect(page).toHaveURL(/\/login\?history-marker=sessions$/);
});

for (const action of ['individual', 'others'] as const) {
  test(`an unauthorized ${action} sign-out clears the account without retrying the mutation`, async ({
    page,
  }) => {
    const state = await fixture(page);
    state.writeStatus = 401;
    let release!: () => void;
    state.holdWrite = new Promise<void>((resolve) => {
      release = resolve;
    });
    await page.goto('/login?history-marker=sessions');
    await expect(page.getByRole('link', { name: 'Session fixture', exact: true })).toBeVisible();
    await page.goto('/settings?tab=account');
    await expect(page.locator('[data-session-client]')).toHaveCount(4);
    const historyLength = await page.evaluate(() => history.length);
    if (action === 'individual') {
      await page.locator('[data-session-client="stable"]').getByRole('button').click();
    } else {
      await page.getByRole('button', { name: '登出其他登入', exact: true }).click();
      await page.getByRole('alertdialog').getByRole('button', { name: '登出', exact: true }).click();
    }
    await expect.poll(() => state.writes.length).toBe(1);
    release();
    await expect(page).toHaveURL(/\/login$/);
    await expect(page.locator('[data-session-client]')).toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Session fixture', exact: true })).toHaveCount(0);
    expect(state.authenticated).toBe(true);
    expect(state.sessions).toHaveLength(4);
    const expected =
      action === 'individual'
        ? 'DELETE /api/v1/me/sessions/stable/00000000-0000-4000-8000-000000000003'
        : 'POST /api/v1/me/sessions/revoke-others';
    expect(state.writes).toEqual([expected]);
    expect(state.apiMutations).toEqual([expected]);
    expect(await page.evaluate(() => history.length)).toBe(historyLength);
    await page.goBack();
    await expect(page).toHaveURL(/\/login\?history-marker=sessions$/);
  });
}
