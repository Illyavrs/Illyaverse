import { test, expect, type Page, type WebSocketRoute } from '@playwright/test';

// All accounts, transcripts and socket events here are synthetic intercepted fixtures.
// This file never changes an existing community account or starts a native game.
type ChatMessage = {
  id: number;
  channel_id: number;
  user_id: number;
  username: string;
  avatar_url: null;
  content: string;
  created_at: string;
  is_action: boolean;
  client_uuid: string | null;
};
const viewer = 98641;
const peer = 98642;
function message(
  id: number,
  channel = 101,
  content = `測試訊息 ${id}`,
  sender = peer,
): ChatMessage {
  return {
    id,
    channel_id: channel,
    user_id: sender,
    username: sender === viewer ? 'Website chat fixture' : 'Illya fixture',
    avatar_url: null,
    content,
    created_at: new Date(Date.UTC(2026, 9, 8, 1, 0, id)).toISOString(),
    is_action: false,
    client_uuid: null,
  };
}

async function fixture(page: Page, locale = 'zh-TW', mode = 'dark', guest = false) {
  const preferences = {
    locale,
    mode,
    accent_color: '#f28eb6',
    background_url: null,
    background_blur: 0,
    background_dim: 0.76,
    panel_blur: 8,
    panel_opacity: 0.55,
    border_radius: 14,
    pm_friends_only: false,
  };
  const user = {
    id: viewer,
    username: 'Website chat fixture',
    email: 'chat@example.test',
    country: 'TW',
    bio: '',
    avatar_url: null,
    banner_url: null,
    accent_color: '#f28eb6',
    banner_position_x: 50,
    banner_position_y: 50,
    roles: [],
    permissions: ['chat.send'],
    joined_at: '2026-10-07T00:00:00Z',
    pp: 0,
    play_count: 0,
  };
  const transcripts: Record<number, ChatMessage[]> = {
    1: [message(900, 1, 'Public channel fixture'), message(901, 1, 'Public follow-up fixture')],
    2: [message(902, 2, 'Second channel fixture')],
    101: Array.from({ length: 205 }, (_, i) => message(i + 1)),
    102: [message(600, 102, 'Second conversation fixture')],
  };
  const state = {
    user,
    sessionExpired: false,
    preferences,
    transcripts,
    sockets: [] as WebSocketRoute[],
    sends: [] as { channel: number; body: Record<string, unknown> }[],
    preferenceWrites: [] as Record<string, unknown>[],
    reads: [] as number[],
    creates: [] as string[],
    hidden: new Set<number>(),
    denied: new Set<number>(),
    canSend: true,
    failCreate: false,
    failPreference: false,
    ambiguousOnce: false,
    gate: null as Promise<void> | null,
    nextId: 1000,
  };
  const conversation = (id: number) => ({
    id,
    peer: {
      id: id === 101 ? peer : peer + 1,
      username: id === 101 ? 'Illya fixture' : 'Miyu fixture',
      avatar_url: null,
    },
    last_message:
      state.transcripts[id]?.filter((item) => !state.hidden.has(item.id)).at(-1) ?? null,
    last_read_id: state.reads.length ? state.reads.at(-1) : 0,
    unread_count: state.reads.length ? 0 : 3,
    can_send: state.canSend,
    send_restriction: state.canSend ? null : 'friend_only',
  });
  const collection = <T>(items: T[], total = items.length) => ({
    items,
    total,
    page: 1,
    page_size: 100,
  });
  await page
    .context()
    .addCookies([{ name: 'illya_locale', value: locale, url: test.info().project.use.baseURL! }]);
  await page.routeWebSocket('**/api/v1/chat/stream', (socket) => {
    state.sockets.push(socket);
    socket.send(JSON.stringify({ type: 'ready' }));
  });
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const pathname = url.pathname.replace('/api/v1', '');
    const method = request.method();
    if (pathname === '/me')
      return guest || state.sessionExpired
        ? route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } })
        : route.fulfill({ json: user });
    if (pathname === '/me/preferences') {
      if (method === 'PATCH') {
        const body = request.postDataJSON();
        state.preferenceWrites.push(body);
        if (state.failPreference)
          return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
        Object.assign(preferences, body);
      }
      return route.fulfill({ json: preferences });
    }
    if (pathname === '/channels')
      return route.fulfill({
        json: collection([
          { id: 1, name: '#general', description: 'Public channel fixture' },
          { id: 2, name: '#help', description: 'Connection help fixture' },
        ]),
      });
    if (pathname === '/conversations' && method === 'GET')
      return route.fulfill({
        json: collection([101, 102].filter((id) => !state.denied.has(id)).map(conversation)),
      });
    if (pathname === '/conversations' && method === 'POST') {
      state.creates.push(request.postDataJSON().target_username);
      if (state.failCreate)
        return route.fulfill({ status: 404, json: { error: { code: 'not_found' } } });
      return route.fulfill({ json: conversation(101) });
    }
    if (pathname === '/chat/visibility') {
      const ids = request.postDataJSON().message_ids as number[];
      expect(ids.length).toBeLessThanOrEqual(256);
      return route.fulfill({ json: { message_ids: ids.filter((id) => !state.hidden.has(id)) } });
    }
    const metadata = /^\/conversations\/(\d+)$/.exec(pathname);
    if (metadata)
      return state.denied.has(Number(metadata[1]))
        ? route.fulfill({ status: 404, json: { error: { code: 'not_found' } } })
        : route.fulfill({ json: conversation(Number(metadata[1])) });
    const read = /^\/conversations\/(\d+)\/read$/.exec(pathname);
    if (read) {
      state.reads.push(request.postDataJSON().message_id);
      return route.fulfill({ status: 204 });
    }
    const history = /^\/(channels|conversations)\/(\d+)\/messages$/.exec(pathname);
    if (history) {
      const channel = Number(history[2]);
      if (state.denied.has(channel))
        return route.fulfill({ status: 404, json: { error: { code: 'not_found' } } });
      if (method === 'POST') {
        const body = request.postDataJSON();
        state.sends.push({ channel, body });
        if (state.gate) await state.gate;
        let persisted = Object.values(transcripts)
          .flat()
          .find((item) => item.client_uuid === body.client_uuid);
        if (!persisted) {
          persisted = {
            ...message(state.nextId++, channel, body.content.trim(), viewer),
            client_uuid: body.client_uuid,
            is_action: body.is_action ?? false,
          };
          state.transcripts[channel].push(persisted);
        }
        if (state.ambiguousOnce) {
          state.ambiguousOnce = false;
          return route.fulfill({ status: 503, json: { error: { code: 'unavailable' } } });
        }
        return route.fulfill({ json: persisted });
      }
      const visible = (transcripts[channel] ?? []).filter((item) => !state.hidden.has(item.id));
      const before = Number(url.searchParams.get('before_id') ?? Infinity);
      const after = Number(url.searchParams.get('after_id') ?? 0);
      return route.fulfill({
        json: collection(
          visible.filter((item) => item.id < before && item.id > after).slice(-100),
          visible.length,
        ),
      });
    }
    return route.fulfill({ status: 404, json: { error: { code: 'fixture_route_missing' } } });
  });
  return state;
}

async function capture(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => {
    document.documentElement.style.setProperty('scroll-behavior', 'auto', 'important');
    (document.activeElement as HTMLElement | null)?.blur();
    window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
  });
  await page.mouse.move(0, 0);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
  if (name.startsWith('mobile-')) {
    const action = await page.locator('.chat-compose, .chat-login').boundingBox();
    expect(action).not.toBeNull();
    expect(action!.y + action!.height).toBeLessThanOrEqual(page.viewportSize()!.height);
    const history = await page.locator('.messages').boundingBox();
    expect(history!.height).toBeGreaterThanOrEqual(80);
  }
  if (process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW === '1')
    await page.screenshot({
      path: `../../.impeccable/review/website-chat-${name}.png`,
      fullPage: true,
      animations: 'disabled',
    });
}

async function selectPrivate(page: Page, id = 101) {
  await expandNavigation(page);
  await page.locator(`.conversation-list button[data-conversation-id="${id}"]`).click();
  await expect(page.locator('.messages')).toHaveAttribute('data-channel-id', String(id));
}

async function expandNavigation(page: Page) {
  await expect(page.locator('.chat-layout')).toBeVisible();
  const toggle = page.locator('.chat-sidebar-toggle button');
  if ((await toggle.isVisible()) && (await toggle.getAttribute('aria-expanded')) === 'false')
    await toggle.click();
}

test('desktop private history, unread acknowledgement, older pages and live lower IDs', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  const state = await fixture(page);
  await page.goto('/chat');
  await selectPrivate(page);
  await expect(page.locator('.messages')).toContainText('測試訊息 205');
  await expect.poll(() => state.reads.includes(205)).toBe(true);
  await page
    .locator('.messages')
    .getByRole('button', { name: '載入較早的訊息', exact: true })
    .click();
  await expect(page.locator('.messages')).toContainText('測試訊息 6');
  await page
    .locator('.messages')
    .getByRole('button', { name: '載入較早的訊息', exact: true })
    .click();
  await expect(page.locator('.messages .message[data-message-id="1"]')).toBeAttached();
  const list = page.locator('.messages');
  await list.evaluate((element) => {
    element.scrollTop = 0;
  });
  const late = message(206, 101, 'Late committed fixture');
  const later = message(207, 101, 'Earlier committed fixture');
  state.transcripts[101].push(late, later);
  state.sockets.at(-1)!.send(JSON.stringify({ type: 'messages', items: [later] }));
  state.sockets.at(-1)!.send(JSON.stringify({ type: 'messages', items: [late] }));
  state.sockets.at(-1)!.send(JSON.stringify({ type: 'messages', items: [late] }));
  await expect(list.locator('.message').filter({ hasText: 'Late committed fixture' })).toHaveCount(
    1,
  );
  await expect.poll(() => list.evaluate((element) => element.scrollTop)).toBe(0);
  const sameUuid = [
    { ...message(208, 101, 'Peer UUID fixture', peer), client_uuid: 'shared-fixture-uuid' },
    { ...message(209, 101, 'Viewer UUID fixture', viewer), client_uuid: 'shared-fixture-uuid' },
  ];
  state.transcripts[101].push(...sameUuid);
  state.sockets.at(-1)!.send(JSON.stringify({ type: 'messages', items: sameUuid }));
  await expect(
    list.locator('.message p').filter({ hasText: /^(Peer|Viewer) UUID fixture$/ }),
  ).toHaveCount(2);
  await capture(page, 'desktop-dark-zhTW-history');
  await expect(page.locator('.global-scene')).toHaveCSS('background-image', 'none');
  await expect(page.locator('body')).toHaveCSS('background-color', 'rgb(0, 0, 0)');
});

test('ambiguous send retry reuses UUID and preserves a newer per-conversation draft', async ({
  page,
}) => {
  const state = await fixture(page, 'en');
  await page.goto('/chat');
  await selectPrivate(page);
  const compose = page.locator('.chat-compose');
  const input = compose.locator('textarea');
  await input.fill('Retry fixture');
  state.ambiguousOnce = true;
  await compose.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(page.locator('.chat-error')).toBeVisible();
  expect(state.sends).toHaveLength(1);
  const uuid = state.sends[0].body.client_uuid;
  expect(uuid).toEqual(expect.any(String));
  let release!: () => void;
  state.gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await compose.getByRole('button', { name: 'Retry send', exact: true }).click();
  await expect.poll(() => state.sends.length).toBe(2);
  await expect(compose.getByRole('button', { name: 'Send', exact: true })).toBeDisabled();
  await input.fill('Newer draft fixture');
  release();
  await expect(input).toHaveValue('Newer draft fixture');
  expect(state.sends[1].body.client_uuid).toBe(uuid);
  expect(state.transcripts[101].filter((item) => item.client_uuid === uuid)).toHaveLength(1);
  await selectPrivate(page, 102);
  await expect(input).toHaveValue('');
  await input.fill('Second draft fixture');
  await selectPrivate(page);
  await expect(input).toHaveValue('Newer draft fixture');
});

test('mobile light new-conversation failure and retry retain recipient input', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await fixture(page, 'en', 'light');
  await page.goto('/chat');
  await expandNavigation(page);
  const form = page.locator('.chat-new-conversation');
  await form.locator('input[name="target_username"]').fill('Illya fixture');
  state.failCreate = true;
  await form.locator('button[type="submit"]').click();
  await expect(form.getByRole('alert')).toBeVisible();
  await expect(form.locator('input')).toHaveValue('Illya fixture');
  await capture(page, 'mobile-light-en-create-error');
  state.failCreate = false;
  await form.locator('button[type="submit"]').click();
  await expect(page.locator('.messages')).toHaveAttribute('data-channel-id', '101');
  expect(state.creates).toEqual(['Illya fixture', 'Illya fixture']);
});

test('Japanese friend-only state and independent account preference failure/retry', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await fixture(page, 'ja');
  state.canSend = false;
  await page.goto('/chat');
  await selectPrivate(page);
  await expect(page.locator('.messages')).toContainText('測試訊息 205');
  await expect(page.locator('.chat-compose button[type="submit"]')).toHaveCount(0);
  await capture(page, 'mobile-dark-ja-friend-only');
  await expandNavigation(page);
  state.failPreference = true;
  await page.locator('#pm_friends_only').check();
  await expect(page.locator('.chat-preferences').getByRole('alert')).toBeVisible();
  state.failPreference = false;
  await page.locator('.chat-preferences button').click();
  await expect.poll(() => state.preferences.pm_friends_only).toBe(true);
  for (const write of state.preferenceWrites)
    expect(Object.keys(write)).toEqual(['pm_friends_only']);
  expect(state.preferences.mode).toBe('dark');
  expect(state.preferences.locale).toBe('ja');
});

test('policy refresh purges cached older messages and inaccessible private conversations', async ({
  page,
}) => {
  const state = await fixture(page, 'en');
  await page.goto('/chat');
  await selectPrivate(page);
  await page
    .locator('.messages')
    .getByRole('button', { name: 'Load older messages', exact: true })
    .click();
  await expect(page.locator('.messages')).toContainText('測試訊息 6');
  state.hidden.add(6);
  state.sockets.at(-1)!.send(JSON.stringify({ type: 'refresh' }));
  await expect(page.locator('.message p').filter({ hasText: /^測試訊息 6$/ })).toHaveCount(0);
  state.denied.add(101);
  state.sockets.at(-1)!.send(JSON.stringify({ type: 'refresh' }));
  await expect(page.locator('.conversation-list button[data-conversation-id="101"]')).toHaveCount(
    0,
  );
  await expect(page.locator('.messages')).not.toContainText('測試訊息 205');
});

test('reconnect refetches history, keeps draft, and guest access exposes only public channels', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  const state = await fixture(page, 'en');
  await page.goto('/chat');
  await selectPrivate(page);
  await page.locator('.chat-compose textarea').fill('Reconnect draft fixture');
  await state.sockets.at(-1)!.close({ code: 1012, reason: 'Synthetic transport interruption' });
  await expect(page.locator('.chat-connection')).toContainText(/reconnect/i);
  await capture(page, 'desktop-dark-en-reconnect');
  state.transcripts[101].push(message(208, 101, 'Reconnect recovered fixture'));
  await expect.poll(() => state.sockets.length).toBeGreaterThan(1);
  await expect(page.locator('.messages')).toContainText('Reconnect recovered fixture');
  await expect(page.locator('.chat-compose textarea')).toHaveValue('Reconnect draft fixture');
  await page.unroute('**/api/v1/**');
  await fixture(page, 'en', 'dark', true);
  await page.reload();
  await expect(page.locator('.messages')).toContainText('Public channel fixture');
  await expect(page.locator('.conversation-list')).toHaveCount(0);
  await expect(page.locator('.chat-new-conversation')).toHaveCount(0);
  await expect(page.locator('.messages')).not.toContainText('Reconnect recovered fixture');
});

test('profile recipient link fills the form without creating or sending until requested', async ({
  page,
}) => {
  const state = await fixture(page, 'en');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/chat?recipient=Illya%20fixture');
  await expect(page.locator('#chat-recipient')).toBeVisible();
  await expect(page.locator('#chat-recipient')).toHaveValue('Illya fixture');
  expect(state.creates).toEqual([]);
  expect(state.sends).toEqual([]);
  await page.getByRole('button', { name: 'Open conversation', exact: true }).click();
  await expect.poll(() => state.creates).toEqual(['Illya fixture']);
  await expect(page.locator('.messages')).toHaveAttribute('data-channel-id', '101');
  expect(state.sends).toEqual([]);
});

test('live permission refresh updates public controls and revoked sessions clear private state', async ({
  page,
}) => {
  const state = await fixture(page, 'en');
  await page.goto('/chat');
  await expect(page.locator('.messages')).toHaveAttribute('data-channel-id', '1');
  await page.locator('.chat-compose textarea').fill('Permission draft fixture');
  state.user.permissions = [];
  state.sockets.at(-1)!.send(JSON.stringify({ type: 'refresh' }));
  await expect(page.locator('.chat-compose')).toHaveCount(0);
  await expect(page.locator('.chat-login')).toContainText('cannot send chat messages');
  state.user.permissions = ['chat.send'];
  state.sockets.at(-1)!.send(JSON.stringify({ type: 'refresh' }));
  await expect(page.locator('.chat-compose textarea')).toHaveValue('Permission draft fixture');
  await selectPrivate(page);
  await expect(page.locator('.messages')).toContainText('測試訊息 205');
  state.sessionExpired = true;
  state.sockets.at(-1)!.send(JSON.stringify({ type: 'refresh' }));
  await expect(page.locator('.conversation-list')).toHaveCount(0);
  await expect(page.locator('.messages')).not.toContainText('測試訊息 205');
  await expect(page.locator('.messages')).toContainText('Public channel fixture');
});
