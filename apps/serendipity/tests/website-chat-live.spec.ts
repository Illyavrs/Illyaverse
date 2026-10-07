import { test, expect, request as apiRequest } from '@playwright/test';
import { createHash, randomBytes } from 'node:crypto';
import path from 'node:path';

function disposableOrigin() {
  const origin = process.env.ILLYA_WEB_URL ?? '';
  if (
    process.env.ILLYA_ALLOW_DISPOSABLE_TEST !== '1' ||
    !/^illya-web-smoke-[a-f0-9]{16}$/.test(process.env.COMPOSE_PROJECT_NAME ?? '') ||
    !process.env.ILLYA_TEST_ENV_FILE ||
    !path.isAbsolute(process.env.ILLYA_TEST_ENV_FILE) ||
    !/^http:\/\/(localhost|127\.0\.0\.1):[1-9][0-9]{3,4}$/.test(origin)
  )
    throw new Error(
      'This writing workflow requires infra/disposable-web-smoke.mjs and its owned local project.',
    );
  return origin;
}
function md5(value: string) {
  return createHash('md5').update(value).digest('hex');
}
function wireString(value: string) {
  const text = Buffer.from(value);
  const length: number[] = [];
  let size = text.length;
  while (size >= 128) {
    length.push((size & 127) | 128);
    size >>= 7;
  }
  length.push(size);
  return Buffer.concat([Buffer.from([11, ...length]), text]);
}
function packet(id: number, body = Buffer.alloc(0)) {
  const header = Buffer.alloc(7);
  header.writeUInt16LE(id);
  header.writeUInt32LE(body.length, 3);
  return Buffer.concat([header, body]);
}
function packets(body: Buffer) {
  const items: { id: number; body: Buffer }[] = [];
  for (let cursor = 0; cursor < body.length;) {
    expect(cursor + 7).toBeLessThanOrEqual(body.length);
    const id = body.readUInt16LE(cursor);
    const size = body.readUInt32LE(cursor + 3);
    expect(body[cursor + 2]).toBe(0);
    expect(cursor + 7 + size).toBeLessThanOrEqual(body.length);
    items.push({ id, body: body.subarray(cursor + 7, cursor + 7 + size) });
    cursor += 7 + size;
  }
  return items;
}

test('final images share website, native HTTP and stable binary private messages', async ({
  page,
}) => {
  test.setTimeout(120000);
  const origin = disposableOrigin();
  const suffix = randomBytes(6).toString('hex');
  const username = `webchat_${suffix}`;
  const peerName = `peerchat_${suffix}`;
  const password = `Chat-fixture-${randomBytes(18).toString('hex')}`;
  await page.context().addCookies([{ name: 'illya_locale', value: 'en', url: origin }]);
  const register = await page.request.post(`${origin}/api/v1/auth/register`, {
    headers: { Origin: origin },
    data: { username, password, email: `${username}@example.test`, locale: 'en' },
  });
  expect(register.ok()).toBeTruthy();
  const actorId = (await register.json()).user.id as number;
  const peerContext = await apiRequest.newContext({
    baseURL: origin,
    extraHTTPHeaders: { Origin: origin },
  });
  try {
    const registered = await peerContext.post('/api/v1/auth/register', {
      data: { username: peerName, password, email: `${peerName}@example.test` },
    });
    expect(registered.ok()).toBeTruthy();
    const peerId = (await registered.json()).user.id as number;
    const hash = `${md5('fixture-path')}:adapter:${md5('fixture-adapter')}:${md5('fixture-uninstall')}:${md5('fixture-disk')}:`;
    const nativeHost = 'c.web-smoke.illya.test';
    const login = await peerContext.post('/', {
      headers: { Host: nativeHost, 'Content-Type': 'application/octet-stream' },
      data: Buffer.from(`${peerName}\n${md5(password)}\nb20261008|8|0|${hash}|0\n`),
    });
    expect(login.ok()).toBeTruthy();
    const token = login.headers()['cho-token'];
    expect(token).toBeTruthy();
    expect(token).not.toBe('no');
    const loginUser = packets(await login.body()).find((item) => item.id === 5)!;
    expect(loginUser.body.readInt32LE()).toBe(peerId);
    const stableHeaders = {
      Host: nativeHost,
      'osu-token': token,
      'Content-Type': 'application/octet-stream',
    };

    await page.goto('/chat');
    await expect(page.locator('html')).toHaveAttribute('lang', 'en');
    const form = page.locator('.chat-new-conversation');
    await form.locator('input[name="target_username"]').fill(peerName);
    await form.locator('button[type="submit"]').click();
    await expect(page.locator('.chat-header')).toContainText(peerName);
    const channel = Number(await page.locator('.messages').getAttribute('data-channel-id'));
    expect(channel).toBeGreaterThan(0);
    await page.locator('.chat-compose textarea').fill(`Website to stable ${suffix}`);
    await page.locator('.chat-compose').getByRole('button', { name: 'Send', exact: true }).click();
    await expect(page.locator('.message p')).toContainText([`Website to stable ${suffix}`]);
    const poll = await peerContext.post('/', { headers: stableHeaders, data: packet(4) });
    expect(poll.ok()).toBeTruthy();
    expect(
      packets(await poll.body()).some(
        (item) =>
          item.id === 7 &&
          item.body.includes(Buffer.from(`Website to stable ${suffix}`)) &&
          item.body.includes(Buffer.from(username)),
      ),
    ).toBe(true);

    const oauth = await peerContext.post('/oauth/token', {
      data: { grant_type: 'password', username: peerName, password },
    });
    expect(oauth.ok()).toBeTruthy();
    const bearer = { Authorization: `Bearer ${(await oauth.json()).access_token}` };
    const nativePost = await peerContext.post(`/api/v2/chat/channels/${channel}/messages`, {
      headers: bearer,
      data: { message: `Native action ${suffix}`, is_action: true, uuid: `native-${suffix}` },
    });
    expect(nativePost.ok()).toBeTruthy();
    await expect(page.locator('.messages')).toContainText(`Native action ${suffix}`);
    const history = await page.request.get(
      `/api/v1/conversations/${channel}/messages?page_size=100`,
    );
    expect(history.ok()).toBeTruthy();
    const nativeMessage = (await history.json()).items.find(
      (item: { content: string }) => item.content === `Native action ${suffix}`,
    );
    expect(nativeMessage.user_id).toBe(peerId);
    expect(nativeMessage.is_action).toBe(true);
    expect(nativeMessage.client_uuid).toBe(`native-${suffix}`);
    const nativeHistory = await peerContext.get(
      `/api/v2/chat/channels/${channel}/messages?limit=50`,
      { headers: bearer },
    );
    expect(nativeHistory.ok()).toBeTruthy();
    expect(
      (await nativeHistory.json()).some(
        (item: { content: string }) => item.content === `Website to stable ${suffix}`,
      ),
    ).toBe(true);

    // Stable currently resolves recipients through native presence; authenticate the
    // synthetic actor in lazer too. This is protocol traffic, not a launched client.
    const actorOAuth = await page.request.post('/oauth/token', {
      headers: { Origin: origin },
      data: { grant_type: 'password', username, password },
    });
    expect(actorOAuth.ok()).toBeTruthy();
    const actorBearer = { Authorization: `Bearer ${(await actorOAuth.json()).access_token}` };
    expect((await page.request.get('/api/v2/me', { headers: actorBearer })).ok()).toBeTruthy();
    const claimed = Buffer.alloc(4);
    claimed.writeInt32LE(999999);
    const direct = Buffer.concat([
      wireString('spoofed-name'),
      wireString(`Stable to website ${suffix}`),
      wireString(username),
      claimed,
    ]);
    expect(
      (await peerContext.post('/', { headers: stableHeaders, data: packet(25, direct) })).ok(),
    ).toBeTruthy();
    await expect(page.locator('.messages')).toContainText(`Stable to website ${suffix}`);
    const bridge = await page.request.get(
      `/api/v1/conversations/${channel}/messages?page_size=100`,
    );
    const stableMessage = (await bridge.json()).items.find(
      (item: { content: string }) => item.content === `Stable to website ${suffix}`,
    );
    expect(stableMessage.user_id).toBe(peerId);
    expect(stableMessage.username).toBe(peerName);

    const sent = await page.request.post(`/api/v1/conversations/${channel}/messages`, {
      headers: { Origin: origin },
      data: { content: `Retry ${suffix}`, is_action: false, client_uuid: `web-${suffix}` },
    });
    expect(sent.ok()).toBeTruthy();
    const original = await sent.json();
    const repeat = await page.request.post(`/api/v1/conversations/${channel}/messages`, {
      headers: { Origin: origin },
      data: { content: `Retry ${suffix}`, is_action: false, client_uuid: `web-${suffix}` },
    });
    expect(repeat.ok()).toBeTruthy();
    expect((await repeat.json()).id).toBe(original.id);
    const conflict = await page.request.post(`/api/v1/conversations/${channel}/messages`, {
      headers: { Origin: origin },
      data: { content: 'Changed retry', client_uuid: `web-${suffix}` },
    });
    expect(conflict.status()).toBe(409);

    const concurrentContent = `Concurrent retry ${suffix}`;
    const concurrentUuid = `concurrent-${suffix}`;
    const [webRetry, nativeRetry] = await Promise.all([
      page.request.post(`/api/v1/conversations/${channel}/messages`, {
        headers: { Origin: origin },
        data: { content: concurrentContent, is_action: false, client_uuid: concurrentUuid },
      }),
      page.request.post(`/api/v2/chat/channels/${channel}/messages`, {
        headers: actorBearer,
        data: { message: concurrentContent, is_action: false, uuid: concurrentUuid },
      }),
    ]);
    expect(webRetry.ok()).toBeTruthy();
    expect(nativeRetry.ok()).toBeTruthy();
    expect((await nativeRetry.json()).message_id).toBe((await webRetry.json()).id);
    const retryHistory = await page.request.get(
      `/api/v1/conversations/${channel}/messages?page_size=100`,
    );
    expect(
      (await retryHistory.json()).items.filter(
        (item: { client_uuid: string }) => item.client_uuid === concurrentUuid,
      ),
    ).toHaveLength(1);

    const block = await peerContext.post(`/api/v1/me/relationships/${actorId}`, {
      data: { relation_type: 'block' },
    });
    expect(block.ok()).toBeTruthy();
    expect((await page.request.get(`/api/v1/conversations/${channel}/messages`)).status()).toBe(
      404,
    );
    await expect(
      page.locator(`.conversation-list button[data-conversation-id="${channel}"]`),
    ).toHaveCount(0, { timeout: 20000 });
    await expect(page.locator('.messages')).not.toContainText(`Stable to website ${suffix}`);
    expect(
      (await peerContext.post('/', { headers: stableHeaders, data: packet(2) })).ok(),
    ).toBeTruthy();
  } finally {
    await peerContext.dispose();
  }
});
