import { test, expect } from '@playwright/test';

// Original, intercepted fixtures; never alter a community account or send a message.
test('profile Message opens a prefilled chat recipient without a POST', async ({ page }) => {
  const stamp = '2026-10-08T00:00:00Z';
  const user = {
    id: 989801,
    username: 'Navigation fixture',
    country: 'TW',
    bio: '',
    avatar_url: null,
    banner_url: null,
    accent_color: '#f28eb6',
    roles: [],
    permissions: ['chat.send'],
    joined_at: stamp,
    pp: 0,
    play_count: 0,
  };
  const target = { ...user, id: 989802, username: 'Recipient fixture' };
  let creates = 0;
  await page
    .context()
    .addCookies([{ name: 'illya_locale', value: 'en', url: test.info().project.use.baseURL! }]);
  await page.routeWebSocket('**/api/v1/chat/stream', (socket) =>
    socket.send(JSON.stringify({ type: 'ready' })),
  );
  await page.route('**/api/v1/**', (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.replace('/api/v1', '');
    if (path === '/me') return route.fulfill({ json: user });
    if (path === '/me/preferences')
      return route.fulfill({ json: { mode: 'dark', locale: 'en', pm_friends_only: false } });
    if (path === `/users/${target.id}`) return route.fulfill({ json: target });
    if (path === '/me/relationships') return route.fulfill({ json: { friends: [], blocks: [] } });
    if (path === '/conversations' && route.request().method() === 'POST') creates++;
    if (path === '/chat/visibility') return route.fulfill({ json: { message_ids: [] } });
    if (path === '/channels')
      return route.fulfill({
        json: {
          items: [{ id: 1, name: '#general', description: 'Fixture channel' }],
          total: 1,
          page: 1,
          page_size: 100,
        },
      });
    return route.fulfill({ json: { items: [], total: 0, page: 1, page_size: 24 } });
  });
  await page.goto(`/players/${target.id}`);
  const message = page.getByRole('link', { name: 'Message', exact: true });
  await expect(message).toHaveAttribute('href', '/chat?recipient=Recipient%20fixture');
  await message.click();
  await expect(page.locator('#chat-recipient')).toHaveValue(target.username);
  expect(creates).toBe(0);
});

test('catalogue preserves profile uploader identity through filters and can clear it', async ({
  page,
}) => {
  const queries: URLSearchParams[] = [];
  await page
    .context()
    .addCookies([{ name: 'illya_locale', value: 'en', url: test.info().project.use.baseURL! }]);
  await page.route('**/api/v1/**', (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === '/api/v1/me')
      return route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } });
    if (url.pathname === '/api/v1/beatmapsets') queries.push(url.searchParams);
    return route.fulfill({ json: { items: [], total: 0, page: 1, page_size: 24 } });
  });
  await page.goto('/beatmaps?uploaded_by=989802&source=local');
  const source = page.getByRole('combobox', { name: 'Source', exact: true });
  await expect(source).toHaveValue('local');
  await expect(page.getByText('Uploaded by player #989802', { exact: true })).toBeVisible();
  await expect.poll(() => queries.at(-1)?.get('uploaded_by')).toBe('989802');
  const beforeSearch = queries.length;
  await page.getByRole('textbox', { name: 'Search', exact: true }).fill('星降り');
  expect(queries).toHaveLength(beforeSearch);
  await page
    .locator('.discovery-search')
    .getByRole('button', { name: 'Search', exact: true })
    .click();
  await expect.poll(() => queries.at(-1)?.get('q')).toBe('星降り');
  expect(queries.at(-1)?.get('uploaded_by')).toBe('989802');
  expect(queries.at(-1)?.get('source')).toBe('local');
  await source.selectOption('official');
  await expect.poll(() => queries.at(-1)?.get('source')).toBe('official');
  expect(queries.at(-1)?.get('uploaded_by')).toBe('989802');
  expect(queries.at(-1)?.get('q')).toBe('星降り');
  await page.goBack();
  await expect(source).toHaveValue('local');
  await expect.poll(() => queries.at(-1)?.get('source')).toBe('local');
  expect(queries.at(-1)?.get('uploaded_by')).toBe('989802');
  expect(queries.at(-1)?.get('q')).toBe('星降り');
  await page.getByRole('button', { name: 'Clear uploader', exact: true }).click();
  await expect.poll(() => queries.at(-1)?.get('uploaded_by')).toBe(null);
  expect(queries.at(-1)?.get('source')).toBe('local');
  expect(queries.at(-1)?.get('q')).toBe('星降り');
  await expect
    .poll(() => Object.fromEntries(new URL(page.url()).searchParams))
    .toEqual({ source: 'local', q: '星降り' });
});
