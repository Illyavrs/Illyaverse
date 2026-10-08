import { expect, test, type Locator, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';

// These fixtures intercept every API request. They never use an account, database,
// or the operator credentials used by the separate community integration suite.
type FixtureRole = {
  id: number;
  name: string;
  description: string;
  permissions: string[];
  is_builtin: boolean;
  membership_count: number;
};
type FixturePlayer = {
  id: number;
  username: string;
  country: string;
  avatar_url: null;
  is_restricted: boolean;
  joined_at: string;
  roles: { id: number; name: string }[];
  can_manage_roles: boolean;
  can_moderate: boolean;
  can_revoke_sessions: boolean;
};
type Write = { method: string; pathname: string; body: Record<string, unknown> | null };
type Reply = { status?: number; json: unknown };
type Fixture = {
  me: {
    id: number;
    username: string;
    permissions: string[];
    roles: string[];
    country: string;
    bio: string;
    avatar_url: null;
    banner_url: null;
    accent_color: string;
    joined_at: string;
    pp: number;
    play_count: number;
  };
  roles: FixtureRole[];
  players: FixturePlayer[];
  writes: Write[];
  directoryQueries: URLSearchParams[];
  roleReads: number;
  meReads: number;
  failDirectoryRefresh: boolean;
  failRoleRefresh: boolean;
  onWrite?: (write: Write, fixture: Fixture) => Reply | undefined;
};
const managerGrants = [
  'admin.view',
  'roles.manage',
  'users.sessions.revoke',
  'chat.send',
  'social.manage',
];
const timestamp = '2026-10-07T00:00:00Z';
const actorId = 91001;
const targetId = 91002;
const hiddenRoleId = 900;

function role(
  id: number,
  name: string,
  permissions: string[],
  is_builtin = false,
  membership_count = 0,
): FixtureRole {
  return { id, name, permissions, is_builtin, membership_count, description: `${name} fixture` };
}
function player(id: number, username: string, roles: FixturePlayer['roles'] = []): FixturePlayer {
  return {
    id,
    username,
    roles,
    country: 'TW',
    avatar_url: null,
    is_restricted: false,
    joined_at: timestamp,
    can_manage_roles: true,
    can_moderate: true,
    can_revoke_sessions: true,
  };
}
const failure = (
  code: string,
  message = 'Untranslated API fixture message',
  status = 400,
): Reply => ({
  status,
  json: { error: { code, message } },
});

async function fixture(page: Page, grants = managerGrants, locale = 'en'): Promise<Fixture> {
  const unexpected: string[] = [];
  const browserErrors: string[] = [];
  const state: Fixture = {
    me: {
      id: actorId,
      username: 'Rbac manager',
      permissions: [...grants],
      roles: ['limited_manager'],
      country: 'TW',
      bio: '',
      avatar_url: null,
      banner_url: null,
      accent_color: '#f28eb6',
      joined_at: timestamp,
      pp: 0,
      play_count: 0,
    },
    roles: [
      role(1, 'player', ['chat.send'], true, 2),
      role(2, 'moderator', ['users.moderate'], true, 1),
      role(3, 'administrator', ['*'], true, 1),
      role(40, 'chat_helper', ['chat.send']),
      role(41, 'assigned_helper', ['social.manage'], false, 1),
      role(42, 'strong_operator', ['users.moderate']),
      role(43, 'custom_wildcard', ['*']),
    ],
    players: [
      player(targetId, 'Rbac target', [
        { id: 1, name: 'player' },
        { id: hiddenRoleId, name: 'archived_helper' },
      ]),
      {
        ...player(91003, 'Stronger operator', [{ id: 42, name: 'strong_operator' }]),
        can_manage_roles: false,
        can_revoke_sessions: false,
        can_moderate: false,
      },
      {
        ...player(91004, 'Wildcard operator', [{ id: 3, name: 'administrator' }]),
        can_manage_roles: false,
        can_revoke_sessions: false,
        can_moderate: false,
      },
    ],
    writes: [],
    directoryQueries: [],
    roleReads: 0,
    meReads: 0,
    failDirectoryRefresh: false,
    failRoleRefresh: false,
  };
  page.on('pageerror', (error) => browserErrors.push(error.message));
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: locale, url: test.info().project.use.baseURL as string },
    ]);
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const pathname = url.pathname.replace('/api/v1', '');
    const method = request.method();
    if (method === 'GET' && pathname === '/me') {
      state.meReads++;
      return route.fulfill({ json: state.me });
    }
    if (pathname === '/me/preferences' && method === 'GET')
      return route.fulfill({
        json: {
          mode: 'dark',
          accent_color: '#f28eb6',
          background_url: null,
          background_blur: 0,
          background_dim: 0.76,
          border_radius: 14,
          locale,
        },
      });
    if (pathname === '/admin/roles' && method === 'GET') {
      state.roleReads++;
      return route.fulfill(
        state.failRoleRefresh
          ? failure('service_unavailable', 'Role refresh fixture failure', 503)
          : {
              json: {
                items: state.roles,
                total: state.roles.length,
                page: 1,
                page_size: state.roles.length,
              },
            },
      );
    }
    if (pathname === '/admin/users' && method === 'GET') {
      state.directoryQueries.push(url.searchParams);
      if (state.failDirectoryRefresh)
        return route.fulfill(
          failure('service_unavailable', 'Directory refresh fixture failure', 503),
        );
      const search = url.searchParams.get('search')?.trim() ?? '';
      const found = state.players.filter(
        (item) =>
          !search ||
          (/^[1-9]\d*$/.test(search)
            ? item.id === Number(search)
            : item.username.toLocaleLowerCase().includes(search.toLocaleLowerCase())),
      );
      const pageNumber = Number(url.searchParams.get('page') ?? 1);
      const size = Number(url.searchParams.get('page_size') ?? 24);
      return route.fulfill({
        json: {
          items: found.slice((pageNumber - 1) * size, pageNumber * size),
          total: found.length,
          page: pageNumber,
          page_size: size,
        },
      });
    }
    if (
      (method === 'PATCH' && /^\/admin\/users\/\d+$/.test(pathname)) ||
      (method === 'POST' && /^\/admin\/users\/\d+\/sessions\/revoke$/.test(pathname)) ||
      (['PATCH', 'DELETE'].includes(method) && /^\/admin\/roles\/\d+$/.test(pathname)) ||
      (method === 'POST' && pathname === '/admin/roles')
    ) {
      const write: Write = {
        method,
        pathname,
        body: request.postData() ? (request.postDataJSON() as Record<string, unknown>) : null,
      };
      state.writes.push(write);
      const override = state.onWrite?.(write, state);
      if (override) return route.fulfill(override);
      if (pathname.endsWith('/sessions/revoke'))
        return route.fulfill({ json: { revoked: { web: 2, stable: 3, lazer: 1 } } });
      if (pathname.startsWith('/admin/users/')) {
        const item = state.players.find((item) => item.id === Number(pathname.split('/').at(-1)));
        if (item && Array.isArray(write.body?.roles))
          item.roles = (write.body.roles as number[]).map((id) => ({
            id,
            name:
              state.roles.find((item) => item.id === id)?.name ??
              item.roles.find((item) => item.id === id)?.name ??
              `role_${id}`,
          }));
        return route.fulfill({ json: { ok: true } });
      }
      if (method === 'DELETE') {
        state.roles = state.roles.filter((item) => item.id !== Number(pathname.split('/').at(-1)));
        return route.fulfill({ json: { ok: true } });
      }
      const id = method === 'POST' ? 44 : Number(pathname.split('/').at(-1));
      const item = {
        ...role(id, String(write.body?.name), write.body?.permissions as string[]),
        description: String(write.body?.description ?? ''),
      };
      state.roles = [...state.roles.filter((item) => item.id !== id), item];
      return route.fulfill({ json: item });
    }
    unexpected.push(`${method} ${pathname}${url.search}`);
    return route.fulfill(
      failure('unexpected_fixture_request', 'No fixture response for this request', 501),
    );
  });
  test.info().annotations.push({
    type: 'fixture-only',
    description: 'All /api/v1 requests intercepted; no real accounts or database.',
  });
  await test.info().attach('fixture-health', {
    body: Buffer.from('Every API request is intercepted by this test.'),
    contentType: 'text/plain',
  });
  // Register per-page assertions after the test, including errors from later refreshes.
  fixtureHealth.set(page, { unexpected, browserErrors });
  return state;
}
const fixtureHealth = new WeakMap<Page, { unexpected: string[]; browserErrors: string[] }>();
test.afterEach(async ({ page }) => {
  const health = fixtureHealth.get(page);
  if (health) {
    expect(health.unexpected, 'Unexpected API requests must not reach a real server').toEqual([]);
    expect(health.browserErrors, 'The fixture flow must not raise browser errors').toEqual([]);
  }
});
const userRow = (page: Page, id = targetId) =>
  page.locator(`.admin-user-row[data-user-id="${id}"]`);
const roleRow = (page: Page, id: number) => page.locator(`.role-row[data-role-id="${id}"]`);
const exactButton = (scope: Page | Locator, name: string) =>
  scope.getByRole('button', { name, exact: true });
async function openAdmin(page: Page) {
  await page.goto('/admin');
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await expect(exactButton(page.locator('.admin-toolbar'), 'Players')).toBeVisible();
  await expect(userRow(page)).toBeVisible();
}
async function openRoles(page: Page) {
  await openAdmin(page);
  await exactButton(page.locator('.admin-toolbar'), 'Roles & permissions').click();
  await expect(page.locator('.admin-role-form')).toBeVisible();
}
async function fillRole(page: Page, name = 'fixture_role') {
  const form = page.locator('.admin-role-form');
  await form.getByLabel('Role name', { exact: true }).fill(name);
  await form.getByLabel('Description', { exact: true }).fill('Unsaved fixture description');
  await form.getByLabel('chat.send', { exact: true }).check();
  return form;
}

test('a role manager sees players and roles, preserves unseen memberships, and cannot restrict', async ({
  page,
}) => {
  const state = await fixture(page, ['admin.view', 'roles.manage', 'chat.send', 'social.manage']);
  await openAdmin(page);
  await expect(exactButton(page.locator('.admin-toolbar'), 'Roles & permissions')).toBeVisible();
  await expect(exactButton(page, 'Restrict')).toHaveCount(0);
  await expect(exactButton(page, 'Revoke sessions')).toHaveCount(0);
  const row = userRow(page);
  await expect(row.getByRole('group', { name: 'Roles for Rbac target' })).toBeVisible();
  await expect(row.getByLabel('archived_helper', { exact: true })).toBeChecked();
  await expect(row.getByLabel('archived_helper', { exact: true })).toBeDisabled();
  await row.getByLabel('chat_helper', { exact: true }).check();
  await exactButton(row, 'Save roles').click();
  await expect.poll(() => state.writes.length).toBe(1);
  expect(state.writes[0]).toMatchObject({ method: 'PATCH', pathname: `/admin/users/${targetId}` });
  expect(state.writes[0].body?.roles).toEqual(expect.arrayContaining([1, hiddenRoleId, 40]));
  expect((state.writes[0].body?.roles as number[]).length).toBe(3);
  await expect(
    page.getByRole('status').filter({ hasText: 'Administrative change saved' }),
  ).toBeVisible();
});

test('a session revoker gets the player directory without role assignment or moderation', async ({
  page,
}) => {
  const state = await fixture(page, ['admin.view', 'users.sessions.revoke']);
  state.players[0].can_manage_roles = false;
  await openAdmin(page);
  const sections = page.locator('.admin-toolbar');
  await expect(exactButton(sections, 'Players')).toBeVisible();
  await expect(exactButton(sections, 'Beatmaps')).toBeVisible();
  for (const name of ['Roles & permissions', 'Scores', 'Audit log', 'Announcements']) {
    await expect(exactButton(sections, name)).toHaveCount(0);
  }
  await expect(sections.locator('button')).toHaveCount(2);
  await expect(exactButton(page, 'Save roles')).toHaveCount(0);
  await expect(exactButton(page, 'Restrict')).toHaveCount(0);
  await expect(exactButton(userRow(page), 'Revoke sessions')).toBeEnabled();
  expect(state.roleReads).toBe(0);
  expect(state.writes).toEqual([]);
});

test('a role draft preserves unseen memberships added by a later player refresh', async ({
  page,
}) => {
  const state = await fixture(page);
  await openAdmin(page);
  const row = userRow(page);
  await row.getByLabel('chat_helper', { exact: true }).check();
  state.players[0].roles.push({ id: 901, name: 'newly_archived' });

  // Revocation refreshes the directory while the independent role draft remains
  // unsaved. The new membership is present only in that server response.
  await exactButton(row, 'Revoke sessions').click();
  await exactButton(
    row.getByRole('group', { name: 'Revoke all sessions for Rbac target?' }),
    'Confirm revocation',
  ).click();
  await expect(row.getByLabel('newly_archived', { exact: true })).toBeChecked();
  await expect(row.getByLabel('newly_archived', { exact: true })).toBeDisabled();
  await expect(row.getByLabel('chat_helper', { exact: true })).toBeChecked();
  await expect(
    page.getByRole('status').filter({ hasText: 'Sessions revoked: web 2, stable 3, lazer 1.' }),
  ).toBeVisible();
  expect(state.writes).toHaveLength(1);

  await exactButton(row, 'Save roles').click();
  await expect.poll(() => state.writes.length).toBe(2);
  expect(state.writes[1]).toEqual({
    method: 'PATCH',
    pathname: `/admin/users/${targetId}`,
    body: { roles: [1, hiddenRoleId, 901, 40] },
  });
  await expect(
    row.getByRole('status').filter({ hasText: 'Administrative change saved' }),
  ).toBeVisible();
});

test('stronger and wildcard roles and targets cannot be delegated by a limited manager', async ({
  page,
}) => {
  const state = await fixture(page);
  await openAdmin(page);
  for (const name of ['strong_operator', 'custom_wildcard', 'administrator'])
    await expect(userRow(page).getByLabel(name, { exact: true })).toBeDisabled();
  for (const id of [91003, 91004]) {
    await expect(exactButton(userRow(page, id), 'Save roles')).toHaveCount(0);
    await expect(exactButton(userRow(page, id), 'Revoke sessions')).toHaveCount(0);
  }
  await exactButton(page.locator('.admin-toolbar'), 'Roles & permissions').click();
  for (const id of [2, 3, 42, 43]) {
    await expect(exactButton(roleRow(page, id), 'Edit role')).toBeDisabled();
    await expect(exactButton(roleRow(page, id), 'Delete role')).toBeDisabled();
  }
  await expect(
    page.locator('.admin-role-form').getByLabel('users.moderate', { exact: true }),
  ).toBeDisabled();
  await expect(
    page.locator('.admin-role-form').getByLabel('users.sessions.revoke', { exact: true }),
  ).toBeEnabled();
  expect(state.writes).toEqual([]);
});

test('builtin names are read only and assigned custom roles cannot be deleted', async ({
  page,
}) => {
  const state = await fixture(page, ['*']);
  await openRoles(page);
  const form = page.locator('.admin-role-form');
  for (const id of [1, 2]) {
    await expect(exactButton(roleRow(page, id), 'Delete role')).toBeDisabled();
    await exactButton(roleRow(page, id), 'Edit role').click();
    await expect(form.getByLabel('Role name', { exact: true })).toHaveAttribute('readonly', '');
    await expect(form.getByLabel('Role name', { exact: true })).toHaveValue(
      state.roles.find((item) => item.id === id)!.name,
    );
    await exactButton(form, 'Cancel').click();
  }
  await expect(exactButton(roleRow(page, 3), 'Edit role')).toBeDisabled();
  await expect(exactButton(roleRow(page, 3), 'Delete role')).toBeDisabled();
  await expect(exactButton(roleRow(page, 41), 'Delete role')).toBeDisabled();
  await exactButton(roleRow(page, 41), 'Edit role').click();
  await expect(form.getByLabel('Role name', { exact: true })).not.toHaveAttribute('readonly', '');
  expect(state.writes).toEqual([]);
});

test('an unassigned delegable role needs a named inline confirmation before deletion', async ({
  page,
}) => {
  const state = await fixture(page);
  await openRoles(page);
  const row = roleRow(page, 40);
  await exactButton(row, 'Delete role').click();
  const confirmation = row.getByRole('group', { name: 'Delete the role “chat_helper”?' });
  await expect(confirmation).toBeVisible();
  expect(state.writes).toEqual([]);
  await exactButton(confirmation, 'Cancel').click();
  await expect(confirmation).toHaveCount(0);
  expect(state.writes).toEqual([]);
  await exactButton(row, 'Delete role').click();
  await exactButton(confirmation, 'Confirm deletion').click();
  await expect.poll(() => state.writes.length).toBe(1);
  expect(state.writes[0]).toEqual({ method: 'DELETE', pathname: '/admin/roles/40', body: null });
  await expect(roleRow(page, 40)).toHaveCount(0);
  await expect(page.getByRole('status').filter({ hasText: 'Role deleted' })).toBeVisible();
});

test('session revocation has a named cancelable inline confirmation and reports each session count', async ({
  page,
}) => {
  const state = await fixture(page, ['admin.view', 'users.sessions.revoke']);
  await openAdmin(page);
  const row = userRow(page);
  const confirmation = row.getByRole('group', { name: 'Revoke all sessions for Rbac target?' });
  await exactButton(row, 'Revoke sessions').click();
  await expect(confirmation).toBeVisible();
  expect(state.writes).toEqual([]);
  await exactButton(confirmation, 'Cancel').click();
  await expect(confirmation).toHaveCount(0);
  await exactButton(row, 'Revoke sessions').click();
  await exactButton(confirmation, 'Confirm revocation').focus();
  await page.keyboard.press('Escape');
  await expect(confirmation).toHaveCount(0);
  expect(state.writes).toEqual([]);
  await exactButton(row, 'Revoke sessions').click();
  await exactButton(confirmation, 'Confirm revocation').click();
  await expect.poll(() => state.writes.length).toBe(1);
  expect(state.writes[0]).toEqual({
    method: 'POST',
    pathname: `/admin/users/${targetId}/sessions/revoke`,
    body: null,
  });
  await expect(
    page.getByRole('status').filter({ hasText: 'Sessions revoked: web 2, stable 3, lazer 1.' }),
  ).toBeVisible();
  expect(state.players[0].is_restricted).toBe(false);
});

test('known role errors are localized, preserve the draft, and unknown 4xx errors retain the API message', async ({
  page,
}) => {
  const state = await fixture(page);
  const cases = [
    ['builtin_role', 'This role is built in. Its name and deletion are protected.'],
    ['last_administrator', 'Keep at least one unrestricted administrator.'],
    [
      'stronger_role',
      'This player or role has permissions you cannot delegate. Refresh your access or ask an administrator.',
    ],
    [
      'role_assigned',
      'This role is assigned to players. Remove its memberships before deleting it.',
    ],
    ['invalid_search', 'Use a username or a positive player ID, up to 64 characters.'],
    [
      'forbidden',
      'Your permissions do not allow this change. Refresh your access or ask an administrator.',
    ],
    ['unauthorized', 'Your session has ended. Sign in again to continue.'],
  ];
  let nextReply = failure(cases[0][0]);
  state.onWrite = () => nextReply;
  await openRoles(page);
  const form = await fillRole(page);
  for (const [code, expected] of cases) {
    nextReply = failure(
      code,
      'This API fixture message must not be shown for a known code',
      code === 'unauthorized' ? 401 : code === 'forbidden' || code === 'stronger_role' ? 403 : 400,
    );
    await exactButton(form, 'Create role').click();
    await expect(form.getByRole('alert')).toHaveText(expected);
    await expect(form.getByLabel('Role name', { exact: true })).toHaveValue('fixture_role');
    await expect(form.getByLabel('Description', { exact: true })).toHaveValue(
      'Unsaved fixture description',
    );
    await expect(form.getByLabel('chat.send', { exact: true })).toBeChecked();
  }
  nextReply = failure(
    'fixture_specific_rule',
    'This fixture rule explains the rejected change.',
    422,
  );
  await exactButton(form, 'Create role').click();
  await expect(form.getByRole('alert')).toHaveText(
    'This fixture rule explains the rejected change.',
  );
  expect(state.writes).toHaveLength(cases.length + 1);
});

test('a successful role write remains successful when the following role refresh fails', async ({
  page,
}) => {
  const state = await fixture(page);
  state.onWrite = (_write, current) => {
    current.failRoleRefresh = true;
    return { json: role(44, 'fixture_role', ['chat.send']) };
  };
  await openRoles(page);
  const form = await fillRole(page);
  await exactButton(form, 'Create role').click();
  await expect(
    page.getByRole('status').filter({ hasText: 'Administrative change saved' }),
  ).toBeVisible();
  await expect(page.locator('.admin-refresh-note')).toHaveText(
    'Change saved. Latest records could not refresh; reload the page before another change.',
  );
  await expect(form.getByRole('alert')).toHaveCount(0);
  await expect(form.getByLabel('Role name', { exact: true })).toHaveValue('');
  expect(state.writes).toHaveLength(1);
  expect(state.roleReads).toBeGreaterThan(1);
});

for (const translation of [
  {
    locale: 'zh-TW',
    roles: '角色與權限',
    name: '角色名稱',
    description: '說明',
    create: '建立角色',
    error: '此玩家或角色含有你無法授予的權限。請重新整理權限，或聯絡管理員。',
  },
  {
    locale: 'ja',
    roles: '役割と権限',
    name: '役割名',
    description: '説明',
    create: '作成',
    error:
      'このプレイヤーまたはロールには、付与できない権限があります。権限を更新するか、管理者にお問い合わせください。',
  },
]) {
  test(`known role errors use ${translation.locale} copy and retain the submitted draft`, async ({
    page,
  }) => {
    const state = await fixture(page, managerGrants, translation.locale);
    state.onWrite = () =>
      failure('stronger_role', 'English backend message must be localized', 403);
    await page.goto('/admin');
    await expect(page.locator('html')).toHaveAttribute('lang', translation.locale);
    await exactButton(page.locator('.admin-toolbar'), translation.roles).click();
    const form = page.locator('.admin-role-form');
    await form.getByLabel(translation.name, { exact: true }).fill('localized_fixture');
    await form
      .getByLabel(translation.description, { exact: true })
      .fill('Retain this localized fixture draft');
    await form.getByLabel('chat.send', { exact: true }).check();
    await exactButton(form, translation.create).click();
    await expect(form.getByRole('alert')).toHaveText(translation.error);
    await expect(form.getByLabel(translation.name, { exact: true })).toHaveValue(
      'localized_fixture',
    );
    await expect(form.getByLabel(translation.description, { exact: true })).toHaveValue(
      'Retain this localized fixture draft',
    );
    await expect(form.getByLabel('chat.send', { exact: true })).toBeChecked();
    expect(state.writes).toHaveLength(1);
  });
}

test('a successful role assignment remains successful when the following directory refresh fails', async ({
  page,
}) => {
  const state = await fixture(page);
  state.onWrite = (_write, current) => {
    current.failDirectoryRefresh = true;
    return { json: { ok: true } };
  };
  await openAdmin(page);
  await userRow(page).getByLabel('chat_helper', { exact: true }).check();
  await exactButton(userRow(page), 'Save roles').click();
  await expect(
    page.getByRole('status').filter({ hasText: 'Administrative change saved' }),
  ).toBeVisible();
  await expect(page.locator('.admin-refresh-note')).toHaveText(
    'Change saved. Latest records could not refresh; reload the page before another change.',
  );
  await expect(page.locator('.form-error[role="alert"]')).toHaveCount(0);
  expect(state.writes).toHaveLength(1);
  expect(state.directoryQueries.length).toBeGreaterThan(1);
});

test('editing the actor role refreshes /me and removes controls for lost grants', async ({
  page,
}) => {
  const state = await fixture(page);
  state.roles.push(role(45, 'limited_manager', [...managerGrants], false, 1));
  state.onWrite = (_write, current) => {
    current.me.permissions = ['admin.view', 'users.sessions.revoke'];
    return { json: role(45, 'limited_manager', current.me.permissions, false, 1) };
  };
  await openRoles(page);
  await exactButton(roleRow(page, 45), 'Edit role').click();
  const form = page.locator('.admin-role-form');
  await form.getByLabel('roles.manage', { exact: true }).uncheck();
  await form.getByLabel('chat.send', { exact: true }).uncheck();
  await form.getByLabel('social.manage', { exact: true }).uncheck();
  await exactButton(form, 'Save changes').click();
  await expect.poll(() => state.meReads).toBeGreaterThan(1);
  await expect(exactButton(page.locator('.admin-toolbar'), 'Roles & permissions')).toHaveCount(0);
  await expect(page.locator('.admin-role-form')).toHaveCount(0);
  await expect(exactButton(page, 'Save roles')).toHaveCount(0);
  await expect(exactButton(userRow(page), 'Revoke sessions')).toBeEnabled();
  expect(state.writes).toHaveLength(1);
  expect(state.writes[0].body?.permissions).toEqual(['admin.view', 'users.sessions.revoke']);
});

test('player search submits literal names and exact IDs, limits length, and resets pagination', async ({
  page,
}) => {
  const state = await fixture(page, ['admin.view', 'users.sessions.revoke']);
  state.players = Array.from({ length: 30 }, (_, index) =>
    player(targetId + index, index === 0 ? 'Literal_% player' : `Directory fixture ${index}`),
  );
  await openAdmin(page);
  const search = page.getByLabel('Find a player', { exact: true });
  await expect(search).toHaveAttribute('maxlength', '64');
  await expect(
    page.getByText('Enter part of a username or an exact player ID. Up to 64 characters.', {
      exact: true,
    }),
  ).toBeVisible();
  await exactButton(page.locator('.pagination'), 'Next').click();
  await expect(page.locator('.pagination')).toContainText('2 / 2');
  await search.fill('Literal_%');
  const beforeSubmit = state.directoryQueries.length;
  await expect(exactButton(page, 'Search')).toBeEnabled();
  expect(state.directoryQueries).toHaveLength(beforeSubmit);
  await exactButton(page, 'Search').click();
  await expect.poll(() => state.directoryQueries.at(-1)?.get('search')).toBe('Literal_%');
  expect(state.directoryQueries.at(-1)?.get('page')).toBe('1');
  await expect(userRow(page)).toContainText('Literal_% player');
  await expect(page.locator('.admin-user-row')).toHaveCount(1);
  await search.fill(String(targetId + 7));
  await exactButton(page, 'Search').click();
  await expect(userRow(page, targetId + 7)).toContainText('Directory fixture 7');
  await exactButton(page, 'Clear search').click();
  await expect(search).toHaveValue('');
  await expect(page.locator('.pagination')).toContainText('1 / 2');
  await search.fill('x'.repeat(65));
  await expect(search).toHaveValue('x'.repeat(64));
  expect(state.directoryQueries.every((query) => (query.get('search')?.length ?? 0) <= 64)).toBe(
    true,
  );
});

test('final admin review captures in Traditional Chinese and Japanese', async ({ page }) => {
  test.skip(
    process.env.ILLYA_CAPTURE_RBAC_REVIEW !== '1',
    'Opt-in final capture batch after functional checks.',
  );
  const reviewDirectory = path.resolve(process.cwd(), '../../data-rbac-review');
  mkdirSync(reviewDirectory, { recursive: true });
  const metrics: Record<string, unknown> = {};
  async function captureReview(name: string) {
    await page.screenshot({ path: path.join(reviewDirectory, `${name}.png`), fullPage: true });
    // Inspect focus and computed layout on the captured view without another render.
    await page.keyboard.press('Tab');
    await page.locator('#admin-user-search, #roleName').first().focus();
    metrics[name] = await page.evaluate(() => {
      const style = (element: Element | null) => {
        if (!element) return null;
        const value = getComputedStyle(element);
        return {
          fontFamily: value.fontFamily,
          fontSize: value.fontSize,
          lineHeight: value.lineHeight,
          color: value.color,
          backgroundColor: value.backgroundColor,
          display: value.display,
          whiteSpace: value.whiteSpace,
          gridTemplateColumns: value.gridTemplateColumns,
          outline: value.outline,
          outlineOffset: value.outlineOffset,
          boxShadow: value.boxShadow,
        };
      };
      const scroll = document.querySelector('.admin-users-scroll');
      const focused = document.activeElement;
      const result = {
        viewportWidth: window.innerWidth,
        documentWidth: document.documentElement.scrollWidth,
        usersTable: scroll
          ? { clientWidth: scroll.clientWidth, scrollWidth: scroll.scrollWidth }
          : null,
        body: style(document.body),
        hint: style(document.querySelector('#admin-search-hint, #role-permission-hint')),
        heading: style(document.querySelector('h1')),
        scrollCue: style(document.querySelector('.admin-scroll-hint')),
        playerId: style(document.querySelector('.admin-player-id')),
        permissionGrid: style(document.querySelector('.admin-role-form .permission-checkboxes')),
        focus: {
          id: focused?.id,
          visible: focused?.matches(':focus-visible'),
          style: style(focused),
        },
      };
      if (focused instanceof HTMLElement) focused.blur();
      return result;
    });
  }
  const state = await fixture(page, ['*'], 'zh-TW');
  state.players = [
    state.players[0],
    player(91005, 'Yuki fixture', [{ id: 41, name: 'assigned_helper' }]),
  ];
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/admin');
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW');
  await expect(userRow(page)).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  await captureReview('desktop-admin-zh-TW');
  await exactButton(page.locator('.admin-toolbar'), '角色與權限').click();
  await expect(page.locator('.admin-role-form')).toBeVisible();
  await captureReview('desktop-roles-zh-TW');
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: 'ja', url: test.info().project.use.baseURL as string },
    ]);
  // Replace the preference response as well so a refresh preserves the selected review locale.
  await page.route('**/api/v1/me/preferences', (route) =>
    route.fulfill({
      json: {
        mode: 'dark',
        accent_color: '#f28eb6',
        background_url: null,
        background_blur: 0,
        background_dim: 0.76,
        border_radius: 14,
        locale: 'ja',
      },
    }),
  );
  await page.setViewportSize({ width: 390, height: 844 });
  // The desktop roles tab persists in the URL; request users for this mobile capture.
  await page.goto('/admin?tab=users');
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
  await expect(userRow(page)).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  await captureReview('mobile-admin-ja');
  await exactButton(page.locator('.admin-toolbar'), '役割と権限').click();
  await expect(page.locator('.admin-role-form')).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  await captureReview('mobile-roles-ja');
  expect(state.writes).toHaveLength(0);
  const metricsJson = JSON.stringify(metrics, null, 2);
  writeFileSync(path.join(reviewDirectory, 'qa-metrics.json'), metricsJson);
  await test.info().attach('admin-review-qa-metrics', {
    body: Buffer.from(metricsJson),
    contentType: 'application/json',
  });
  console.log(`RBAC review metrics: ${JSON.stringify(metrics)}`);
});
