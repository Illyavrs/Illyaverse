import { expect, test, request as apiRequest, type Page } from '@playwright/test';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import type { Announcement, AnnouncementStatus } from '../src/lib/types';

const stamp = '2026-10-08T04:00:00Z';
const author = { id: 990801, username: 'Announcement fixture editor', avatar_url: null };
const synthetic = 'Synthetic announcement fixtures; no real account or database is used.';
const captured: Record<string, unknown>[] = [];
const copy = {
  'zh-TW': {
    title: '伺服器維護時間與連線方式',
    body: '這是供介面測試使用的公告範例。\n\n維護期間請先結束遊戲，再依連線教學重新啟動客戶端。\n若連線失敗，請確認帳號與伺服器位址。',
    draft: '尚未發布的維護草稿',
  },
  en: {
    title: 'Server maintenance and connection details',
    body: 'This is a synthetic announcement for interface testing.\n\nDuring maintenance, finish your game and follow the connection guide to restart your client.\nIf the connection fails, check your account and server address.',
    draft: 'Unpublished maintenance draft',
  },
  ja: {
    title: 'サーバーメンテナンスと接続方法',
    body: 'これは画面テスト用のお知らせです。\n\nメンテナンス中はゲームを終了し、接続ガイドに沿ってクライアントを再起動してください。\n接続できない場合はアカウントとサーバーアドレスを確認してください。',
    draft: '未公開のメンテナンス下書き',
  },
};

function announcement(
  id: number,
  status: AnnouncementStatus,
  title: string,
  body: string,
): Announcement {
  return {
    id,
    status,
    title,
    body,
    revision: 1,
    created_at: stamp,
    updated_at: stamp,
    published_at: status === 'published' ? stamp : null,
    author,
  };
}

async function fixture(
  page: Page,
  locale: keyof typeof copy = 'en',
  mode = 'dark',
  permissions = ['admin.view', 'announcements.manage'],
) {
  const text = copy[locale];
  const state = {
    items: [
      announcement(990900, 'draft', text.draft, 'Private draft body — synthetic fixture.'),
      announcement(990899, 'published', text.title, text.body),
      ...Array.from({ length: 24 }, (_, index) =>
        announcement(990898 - index, 'published', `${text.title} ${index + 2}`, text.body),
      ),
      announcement(990700, 'archived', 'Withdrawn synthetic announcement', 'Archived private text'),
    ],
    permissions: [...permissions],
    viewer: author.id as number | null,
    publicError: 0,
    adminError: 0,
    failWrite: 0,
    conflict: false,
    injectDraft: false,
    writes: [] as { method: string; id?: number; body: Record<string, unknown> }[],
    queries: [] as string[],
    privateReads: 0,
    privateDetailReads: 0,
    meReads: 0,
    holdWrite: false,
    releaseWrite: null as (() => void) | null,
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
  await page
    .context()
    .addCookies([
      { name: 'illya_locale', value: locale, url: test.info().project.use.baseURL as string },
    ]);
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const resource = url.pathname.replace('/api/v1', '');
    const fail = (status: number, code = 'fixture_failure') =>
      route.fulfill({
        status,
        json: { error: { code, message: 'Raw untranslated fixture error' } },
      });
    const collection = (items: Announcement[]) => {
      const currentPage = Number(url.searchParams.get('page') ?? 1);
      const pageSize = Number(url.searchParams.get('page_size') ?? 24);
      return {
        items: items.slice((currentPage - 1) * pageSize, currentPage * pageSize),
        total: items.length,
        page: currentPage,
        page_size: pageSize,
      };
    };
    if (resource === '/me') {
      state.meReads++;
      return state.viewer === null
        ? fail(401, 'unauthorized')
        : route.fulfill({
            json: {
              id: state.viewer,
              username: author.username,
              country: 'TW',
              bio: '',
              avatar_url: null,
              banner_url: null,
              accent_color: '#f28eb6',
              roles: [],
              permissions: state.permissions,
              joined_at: stamp,
              pp: 0,
              play_count: 0,
            },
          });
    }
    if (resource === '/me/preferences') return route.fulfill({ json: preferences });
    if (resource === '/auth/logout') {
      state.viewer = null;
      return route.fulfill({ status: 204 });
    }
    if (resource === '/announcements') {
      state.queries.push(url.search);
      if (state.publicError) return fail(state.publicError);
      const result = collection(state.items.filter((item) => item.status === 'published'));
      if (state.injectDraft) result.items.unshift(state.items[0]);
      return route.fulfill({ json: result });
    }
    const publicDetail = /^\/announcements\/(\d+)$/.exec(resource);
    if (publicDetail) {
      if (state.publicError) return fail(state.publicError);
      const item = state.items.find(
        (value) => value.id === Number(publicDetail[1]) && value.status === 'published',
      );
      return item ? route.fulfill({ json: item }) : fail(404, 'not_found');
    }
    const privateDetail = /^\/admin\/announcements\/(\d+)$/.exec(resource);
    if (resource === '/admin/announcements' || privateDetail) {
      if (state.viewer === null) return fail(401, 'unauthorized');
      if (!state.permissions.includes('announcements.manage') && !state.permissions.includes('*'))
        return fail(403, 'forbidden');
      if (request.method() === 'GET') {
        state.privateReads++;
        if (state.adminError) return fail(state.adminError);
        if (privateDetail) {
          state.privateDetailReads++;
          const item = state.items.find((value) => value.id === Number(privateDetail[1]));
          return item ? route.fulfill({ json: item }) : fail(404, 'not_found');
        }
        const status = url.searchParams.get('status') ?? 'all';
        state.queries.push(url.search);
        return route.fulfill({
          json: collection(
            state.items.filter((item) => status === 'all' || item.status === status),
          ),
        });
      }
      const body = request.postDataJSON();
      if (state.holdWrite)
        await new Promise<void>((resolve) => {
          state.releaseWrite = resolve;
        });
      if (state.failWrite) return fail(state.failWrite);
      if (request.method() === 'POST') {
        state.writes.push({ method: 'POST', body });
        const created = announcement(991000 + state.writes.length, 'draft', body.title, body.body);
        state.items.unshift(created);
        return route.fulfill({ status: 201, json: created });
      }
      if (privateDetail && request.method() === 'PATCH') {
        const item = state.items.find((value) => value.id === Number(privateDetail[1]));
        if (!item) return fail(404, 'not_found');
        if (state.conflict) {
          state.conflict = false;
          item.revision++;
          item.body = 'Latest text from another editor — synthetic fixture.';
          return fail(409, 'announcement_conflict');
        }
        if (body.revision !== item.revision) return fail(409, 'announcement_conflict');
        state.writes.push({ method: 'PATCH', id: item.id, body });
        Object.assign(item, {
          title: body.title,
          body: body.body,
          status: body.status,
          revision: item.revision + 1,
          published_at:
            body.status === 'published' ? (item.published_at ?? stamp) : item.published_at,
        });
        return route.fulfill({ json: item });
      }
    }
    if (resource === '/status')
      return route.fulfill({
        json: {
          players_online: 2,
          registered_players: 12,
          ranked_scores: 8,
          beatmap_count: 3,
          compatibility: {},
        },
      });
    if (resource === '/beatmaps')
      return route.fulfill({
        json: {
          items: [
            {
              id: 990600,
              title: 'Synthetic beatmap for announcement review',
              artist: 'Fixture artist',
              creator: 'Fixture mapper',
              version: 'Normal',
              mode: 0,
              status: 'ranked',
              source: 'local',
              stars: 3.25,
              length_seconds: 120,
              bpm: 180,
              max_combo: 200,
              play_count: 0,
              official_id: null,
              md5: '0'.repeat(32),
              download_url: '/api/v1/beatmaps/990600/download',
              download_kind: 'osu',
              package: null,
            },
          ],
          total: 1,
          page: 1,
          page_size: 3,
        },
      });
    if (resource === '/leaderboard')
      return route.fulfill({
        json: {
          items: [
            {
              id: author.id,
              username: author.username,
              country: 'TW',
              avatar_url: null,
              pp: 123,
              rank: 1,
              play_count: 8,
              accuracy: 98,
            },
          ],
          total: 1,
          page: 1,
          page_size: 5,
        },
      });
    if (resource === '/scores' || resource === '/admin/beatmaps' || resource === '/admin/roles')
      return route.fulfill({ json: { items: [], total: 0, page: 1, page_size: 24 } });
    return fail(404, 'fixture_missing');
  });
  return state;
}

async function capture(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => window.scrollTo({ top: 0, left: 0, behavior: 'instant' }));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
  if (process.env.ILLYA_CAPTURE_ANNOUNCEMENTS_REVIEW !== '1') return;
  const directory = path.resolve('../../.impeccable/review');
  mkdirSync(directory, { recursive: true });
  const filename = `announcements-${name}.png`;
  await page.screenshot({
    path: path.join(directory, filename),
    fullPage: true,
    animations: 'disabled',
  });
  captured.push({
    filename,
    viewport: page.viewportSize(),
    route: new URL(page.url()).pathname + new URL(page.url()).search,
    fixture: synthetic,
    ...(await page.evaluate(() => ({
      locale: document.documentElement.lang,
      theme: document.documentElement.classList.contains('light') ? 'light' : 'dark',
      scrollY: window.scrollY,
      overflow: document.documentElement.scrollWidth > innerWidth,
    }))),
  });
  writeFileSync(
    path.join(directory, 'announcements-captures.json'),
    JSON.stringify({ fixtures: synthetic, captures: captured }, null, 2),
  );
}

test('published public rows paginate and the homepage excludes injected private fixtures', async ({
  page,
}) => {
  const state = await fixture(page, 'zh-TW');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/news');
  await expect(page.locator('.page-heading h1')).toHaveText('公告');
  await expect(page.locator('.announcement-rows > li')).toHaveCount(12);
  await expect(page.locator('.announcement-author').first()).toHaveAttribute(
    'href',
    `/players/${author.id}`,
  );
  await expect(page.locator('.announcement-rows')).not.toContainText(copy['zh-TW'].draft);
  await capture(page, 'news-1440-dark-zh-TW-populated');
  await page.locator('.pagination button').last().click();
  await expect(page).toHaveURL(/page=2/);
  await expect(page.locator('.announcement-rows > li')).toHaveCount(12);
  await page.locator('.pagination button').last().click();
  await expect(page.locator('.announcement-rows > li')).toHaveCount(1);
  await page.goBack();
  await expect(page.locator('.announcement-rows > li')).toHaveCount(12);
  state.injectDraft = true;
  await page.goto('/');
  await expect(page.locator('.announcements-preview .announcement-rows > li')).toHaveCount(3);
  await expect(page.locator('.announcements-preview')).not.toContainText(copy['zh-TW'].draft);
  await expect(page.locator('.announcements-preview .text-link')).toHaveAttribute('href', '/news');
  await expect(page.locator('.home-community .loading-block')).toHaveCount(0);
  await capture(page, 'home-1440-dark-zh-TW-populated');
});

test('staff navigation stays contained at tablet and narrow phone widths', async ({ page }) => {
  await fixture(page);
  await page.goto('/news');
  for (const width of [1440, 1000, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    await expect
      .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
      .toBe(true);
    expect(
      await page.locator('.header-inner').evaluate((header) =>
        Array.from(header.children).every((child) => {
          const bounds = child.getBoundingClientRect();
          return bounds.width === 0 || (bounds.left >= 0 && bounds.right <= innerWidth);
        }),
      ),
    ).toBe(true);
    if (width <= 1280) {
      const menu = page.getByRole('button', { name: 'Open navigation', exact: true });
      await menu.focus();
      await page.keyboard.press('Enter');
      await expect(
        page.locator('.mobile-nav').getByRole('link', { name: 'Announcements', exact: true }),
      ).toBeVisible();
      await expect(
        page.locator('.mobile-nav').getByRole('link', { name: 'Administration', exact: true }),
      ).toBeVisible();
      await page.keyboard.press('Escape');
      await expect(menu).toBeFocused();
    }
  }
});

test('custom role editor exposes the announcement management grant', async ({ page }) => {
  await fixture(page, 'en', 'dark', ['admin.view', 'roles.manage', 'announcements.manage']);
  await page.goto('/admin?tab=roles');
  const grant = page.getByRole('checkbox', { name: 'announcements.manage', exact: true });
  await expect(grant).toBeEnabled();
  await grant.check();
  await expect(grant).toBeChecked();
});

test('mobile light detail preserves long plain text and supports keyboard navigation', async ({
  page,
}) => {
  const state = await fixture(page, 'en', 'light');
  const body = `<img src=x onerror="window.fixtureExecuted=true">\n\n${copy.en.body}\n\n${'Long unbroken fixture text '.repeat(28)}\n${'x'.repeat(240)}\n\nFinal line.`;
  state.items[1].body = body;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/news/990899');
  await expect(page.locator('html')).toHaveClass(/light/);
  await expect(page.locator('.announcement-body')).toHaveText(body);
  await expect(page.locator('.announcement-body img')).toHaveCount(0);
  await expect(page.locator('.announcement-body')).toHaveCSS('white-space', 'pre-wrap');
  expect(await page.evaluate(() => 'fixtureExecuted' in window)).toBe(false);
  await capture(page, 'news-390-light-en-longbody');
  await page.locator('.announcement-back').focus();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/\/news$/);
  await page.getByRole('button', { name: 'Open navigation', exact: true }).click();
  await expect(
    page.locator('.mobile-nav').getByRole('link', { name: 'Announcements', exact: true }),
  ).toBeVisible();
});

test('create, failed save, explicit publish and withdrawal retain the authored text', async ({
  page,
}) => {
  const state = await fixture(page);
  await page.goto('/admin?tab=announcements');
  await page.getByRole('button', { name: 'New announcement', exact: true }).click();
  await page.getByLabel('Title', { exact: true }).fill('Authored synthetic announcement');
  await page.getByLabel('Body', { exact: true }).fill('First line\nSecond line');
  state.failWrite = 503;
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect(page.locator('.announcement-editor .form-error')).toContainText(
    "The server couldn't complete your request.",
  );
  await expect(page.getByLabel('Body', { exact: true })).toHaveValue('First line\nSecond line');
  expect(state.writes).toHaveLength(0);
  state.failWrite = 0;
  state.holdWrite = true;
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect.poll(() => typeof state.releaseWrite).toBe('function');
  await expect(page.getByRole('button', { name: 'Publish', exact: true })).toBeDisabled();
  state.holdWrite = false;
  state.releaseWrite?.();
  await expect(page.locator('.announcement-editor .form-success')).toContainText('Draft saved');
  expect(state.writes).toHaveLength(1);
  expect(state.writes[0].method).toBe('POST');
  await page.getByRole('button', { name: 'Publish', exact: true }).click();
  await expect(page.locator('.announcement-editor .form-success')).toHaveText(
    'Announcement published.',
  );
  expect(state.writes[1].body.status).toBe('published');
  await page.getByRole('button', { name: 'Withdraw', exact: true }).click();
  await page
    .getByRole('group', { name: 'Withdraw this announcement?' })
    .getByRole('button', { name: 'Withdraw', exact: true })
    .click();
  await expect(page.locator('.announcement-editor .form-success')).toHaveText(
    'Announcement withdrawn from the public website.',
  );
  expect(state.writes[2].body.status).toBe('archived');
  expect(state.writes[2].body.body).toBe('First line\nSecond line');
  await page.goto(`/news/${state.writes[2].id}`);
  await expect(page.locator('main')).toContainText('This announcement is unavailable.');
  await expect(page.locator('.announcement-article')).toHaveCount(0);
});

test('desktop editor guards selection, cancellation and tab navigation without implicit writes', async ({
  page,
}) => {
  const state = await fixture(page, 'zh-TW');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/admin');
  await page.locator('.admin-toolbar').getByRole('button', { name: '公告', exact: true }).click();
  await page.locator('.announcement-select').first().click();
  await page.getByLabel('標題', { exact: true }).fill('維護草稿：請確認連線教學與伺服器位址');
  await page.getByLabel('內文', { exact: true }).fill(copy['zh-TW'].body);
  await page.goBack();
  await expect(page).toHaveURL(/\/admin$/);
  await page.goForward();
  await expect(page.getByLabel('標題', { exact: true })).toHaveValue(
    '維護草稿：請確認連線教學與伺服器位址',
  );
  await expect(page.getByLabel('內文', { exact: true })).toHaveValue(copy['zh-TW'].body);
  await expect(page.locator('.announcement-recovered')).toContainText('已恢復');
  await expect(page.locator('.announcement-admin-rows > li')).toHaveCount(12);
  await capture(page, 'admin-1440-dark-zh-TW-edit');
  await page.locator('.admin-toolbar').getByRole('button', { name: '圖譜', exact: true }).click();
  await expect(page.getByText('要捨棄尚未儲存的變更嗎？', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '繼續編輯', exact: true }).click();
  await expect(page).toHaveURL(/tab=announcements/);
  await page.locator('.announcement-select').nth(1).click();
  await expect(page.locator('.announcement-confirm')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.announcement-confirm')).toHaveCount(0);
  await expect(page.getByLabel('標題', { exact: true })).toHaveValue(
    '維護草稿：請確認連線教學與伺服器位址',
  );
  await page
    .locator('.announcement-editor-actions')
    .getByRole('button', { name: '取消', exact: true })
    .click();
  await page.getByRole('button', { name: '捨棄變更', exact: true }).click();
  await expect(page.locator('.announcement-editor')).toHaveCount(0);
  expect(state.writes).toHaveLength(0);
});

test('mobile Japanese conflict keeps unsaved text until latest replacement is confirmed', async ({
  page,
}) => {
  const state = await fixture(page, 'ja');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/admin?tab=announcements');
  await page.locator('.announcement-select').first().click();
  await page
    .getByLabel('本文', { exact: true })
    .fill('自分の未保存の変更。\n確認してから公開します。');
  state.conflict = true;
  await page.getByRole('button', { name: '下書きを保存', exact: true }).click();
  await expect(page.locator('.announcement-editor .form-error')).toContainText('別の編集者');
  await expect(page.getByLabel('本文', { exact: true })).toHaveValue(
    '自分の未保存の変更。\n確認してから公開します。',
  );
  await expect(page.getByRole('button', { name: '公開する', exact: true })).toBeDisabled();
  await capture(page, 'admin-390-dark-ja-conflict');
  await page
    .locator('.announcement-conflict')
    .getByRole('button', { name: '最新の版を読み込む', exact: true })
    .click();
  await page.getByRole('button', { name: '編集を続ける', exact: true }).click();
  expect(state.privateDetailReads).toBe(0);
  await expect(page.getByLabel('本文', { exact: true })).toHaveValue(
    '自分の未保存の変更。\n確認してから公開します。',
  );
  await page
    .locator('.announcement-conflict')
    .getByRole('button', { name: '最新の版を読み込む', exact: true })
    .click();
  await page
    .getByRole('group', { name: '最新の版で入力内容を置き換えますか？' })
    .getByRole('button', { name: '最新の版を読み込む', exact: true })
    .click();
  await expect(page.getByLabel('本文', { exact: true })).toHaveValue(
    'Latest text from another editor — synthetic fixture.',
  );
  expect(state.privateDetailReads).toBe(1);
  await page.getByRole('button', { name: '下書きを保存', exact: true }).click();
  await expect(page.locator('.announcement-editor .form-success')).toContainText(
    '下書きを保存しました',
  );
  expect(state.writes[0].body.revision).toBe(2);
});

test('admin pagination and status filters preserve the open editor', async ({ page }) => {
  const state = await fixture(page);
  await page.goto('/admin?tab=announcements');
  await expect(page.locator('.announcement-admin-rows > li')).toHaveCount(12);
  await page.locator('.announcement-select').first().click();
  await page.getByLabel('Body', { exact: true }).fill('Keep this local draft while browsing.');
  await page.locator('.announcement-management-list .pagination button').last().click();
  await expect(page).toHaveURL(/page=2/);
  await expect(page.getByLabel('Body', { exact: true })).toHaveValue(
    'Keep this local draft while browsing.',
  );
  await page.getByLabel('Publication status', { exact: true }).selectOption('draft');
  await expect(page.locator('.announcement-admin-rows > li')).toHaveCount(1);
  await expect(page).toHaveURL(/status=draft/);
  expect(new URL(page.url()).searchParams.has('page')).toBe(false);
  await expect(page.getByLabel('Body', { exact: true })).toHaveValue(
    'Keep this local draft while browsing.',
  );
  await page.goBack();
  await expect(page).toHaveURL(/page=2/);
  await expect(page.getByLabel('Body', { exact: true })).toHaveValue(
    'Keep this local draft while browsing.',
  );
  await page.goForward();
  await expect(page).toHaveURL(/status=draft/);
  await expect(page.getByLabel('Body', { exact: true })).toHaveValue(
    'Keep this local draft while browsing.',
  );
  await expect(page.locator('.announcement-recovered')).toHaveCount(0);
  expect(state.writes).toHaveLength(0);
});

async function enterEditorThroughNavigation(page: Page) {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/news');
  await page
    .locator('.desktop-nav')
    .getByRole('link', { name: 'Administration', exact: true })
    .click();
  await expect(page).toHaveURL(/\/admin$/);
  await page
    .locator('.admin-toolbar')
    .getByRole('button', { name: 'Announcements', exact: true })
    .click();
  await expect(page).toHaveURL(/tab=announcements/);
}

async function refreshViewer(page: Page, state: { meReads: number }) {
  const reads = state.meReads;
  await expect
    .poll(
      async () => {
        await page.evaluate(() => window.dispatchEvent(new Event('focus')));
        return state.meReads;
      },
      { timeout: 15000, intervals: [500, 1000] },
    )
    .toBeGreaterThan(reads);
}

test('Back and Forward recover an unpublished draft across announcement tabs and routes without writes', async ({
  page,
}) => {
  const state = await fixture(page);
  await enterEditorThroughNavigation(page);
  await page.getByRole('button', { name: 'New announcement', exact: true }).click();
  await page.getByLabel('Title', { exact: true }).fill('Own unpublished history draft');
  await page
    .getByLabel('Body', { exact: true })
    .fill('Keep this exact unsaved text.\nIt has not been published.');
  await page.goBack();
  await expect(page).toHaveURL(/\/admin$/);
  await expect(page.locator('.announcement-editor')).toHaveCount(0);
  await page.goForward();
  await expect(page.getByLabel('Title', { exact: true })).toHaveValue(
    'Own unpublished history draft',
  );
  await expect(page.getByLabel('Body', { exact: true })).toHaveValue(
    'Keep this exact unsaved text.\nIt has not been published.',
  );
  await expect(page.locator('.announcement-recovered')).toContainText(
    'have not been saved to the server',
  );
  expect(state.writes).toHaveLength(0);
  await page.goBack();
  await page.goBack();
  await expect(page).toHaveURL(/\/news$/);
  await page.goForward();
  await page.goForward();
  await expect(page.getByLabel('Title', { exact: true })).toHaveValue(
    'Own unpublished history draft',
  );
  await expect(page.getByLabel('Body', { exact: true })).toHaveValue(
    'Keep this exact unsaved text.\nIt has not been published.',
  );
  await page.getByRole('button', { name: 'New announcement', exact: true }).click();
  await page.getByRole('button', { name: 'Keep editing', exact: true }).click();
  await expect(page.getByLabel('Title', { exact: true })).toHaveValue(
    'Own unpublished history draft',
  );
  await expect(page.locator('.announcement-recovered')).toBeVisible();
  await page
    .locator('.announcement-editor-actions')
    .getByRole('button', { name: 'Cancel', exact: true })
    .click();
  await page.getByRole('button', { name: 'Discard changes', exact: true }).click();
  await expect(page.locator('.announcement-editor')).toHaveCount(0);
  await page.goBack();
  await page.goForward();
  await expect(page.locator('.announcement-editor')).toHaveCount(0);
  expect(state.writes).toHaveLength(0);
});

test('history recovery keeps own text when the original record changed or disappeared', async ({
  page,
}) => {
  const state = await fixture(page);
  await enterEditorThroughNavigation(page);
  await page.locator('.announcement-select').first().click();
  await page.getByLabel('Title', { exact: true }).fill('Own title across a concurrent update');
  await page.getByLabel('Body', { exact: true }).fill('Own body across a concurrent update');
  await page.goBack();
  await expect(page).toHaveURL(/\/admin$/);
  state.items[0].revision++;
  state.items[0].status = 'archived';
  state.items[0].body = 'Different text from another editor';
  await page.goForward();
  await expect(page.getByLabel('Title', { exact: true })).toHaveValue(
    'Own title across a concurrent update',
  );
  await expect(page.getByLabel('Body', { exact: true })).toHaveValue(
    'Own body across a concurrent update',
  );
  await expect(page.locator('.announcement-editor .form-error')).toContainText(
    'changed by another editor',
  );
  await expect(page.getByRole('button', { name: 'Save draft', exact: true })).toBeDisabled();
  await expect(page.locator('.announcement-recovered')).toBeVisible();
  await page.goBack();
  await expect(page).toHaveURL(/\/admin$/);
  state.items = state.items.filter((item) => item.id !== 990900);
  await page.goForward();
  await expect(page.getByLabel('Body', { exact: true })).toHaveValue(
    'Own body across a concurrent update',
  );
  await expect(page.locator('.announcement-editor .form-error')).toHaveText(
    'This announcement is unavailable.',
  );
  await expect(page.getByRole('button', { name: 'Publish', exact: true })).toBeDisabled();
  expect(state.writes).toHaveLength(0);
});

for (const change of ['lost grant', 'changed account', 'logout'] as const) {
  test(`retained history text clears after ${change} while the editor is absent`, async ({
    page,
  }) => {
    const state = await fixture(page);
    await enterEditorThroughNavigation(page);
    await page.getByRole('button', { name: 'New announcement', exact: true }).click();
    await page.getByLabel('Title', { exact: true }).fill(`Private title before ${change}`);
    await page.getByLabel('Body', { exact: true }).fill(`Private text before ${change}`);
    await page.goBack();
    await page.goBack();
    await expect(page).toHaveURL(/\/news$/);
    if (change === 'lost grant') {
      state.permissions = ['admin.view'];
      await refreshViewer(page, state);
      await page
        .locator('.desktop-nav')
        .getByRole('link', { name: 'Administration', exact: true })
        .click();
      await expect(
        page.locator('.admin-toolbar').getByRole('button', { name: 'Announcements', exact: true }),
      ).toHaveCount(0);
      state.permissions = ['admin.view', 'announcements.manage'];
      await refreshViewer(page, state);
      await page
        .locator('.admin-toolbar')
        .getByRole('button', { name: 'Announcements', exact: true })
        .click();
    } else if (change === 'changed account') {
      state.viewer = author.id + 1;
      await refreshViewer(page, state);
      await expect(page.locator('.user-link')).toHaveAttribute('href', `/players/${author.id + 1}`);
      await page.goForward();
      await page.goForward();
    } else {
      await page.getByRole('button', { name: 'Sign out', exact: true }).click();
      await expect(page).toHaveURL(/\/$/);
      state.viewer = author.id;
      await refreshViewer(page, state);
      await page
        .locator('.desktop-nav')
        .getByRole('link', { name: 'Administration', exact: true })
        .click();
      await page
        .locator('.admin-toolbar')
        .getByRole('button', { name: 'Announcements', exact: true })
        .click();
    }
    await expect(page).toHaveURL(/tab=announcements/);
    await expect(page.locator('.announcement-select').first()).toBeVisible();
    await expect(page.locator('.announcement-editor')).toHaveCount(0);
    await page.getByRole('button', { name: 'New announcement', exact: true }).click();
    await expect(page.getByLabel('Title', { exact: true })).toHaveValue('');
    await expect(page.getByLabel('Body', { exact: true })).toHaveValue('');
    await expect(page.locator('.announcement-recovered')).toHaveCount(0);
    expect(state.writes).toHaveLength(0);
  });
}

test('missing grant hides the tab and expired grant clears private forms and cached lists', async ({
  page,
}) => {
  const state = await fixture(page, 'en', 'dark', ['admin.view']);
  await page.goto('/admin?tab=announcements');
  await expect(
    page.locator('.admin-toolbar').getByRole('button', { name: 'Announcements', exact: true }),
  ).toHaveCount(0);
  expect(state.privateReads).toBe(0);
  state.permissions.push('announcements.manage');
  await page.reload();
  await page
    .locator('.admin-toolbar')
    .getByRole('button', { name: 'Announcements', exact: true })
    .click();
  await page.locator('.announcement-select').first().click();
  await page.getByLabel('Body', { exact: true }).fill('Private unsaved text after grant expiry');
  state.permissions = ['admin.view'];
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect(page.locator('#announcement-body')).toHaveCount(0);
  await expect(page.locator('main')).not.toContainText('Private unsaved text after grant expiry');
  await expect(page.locator('main')).not.toContainText(copy.en.draft);
  await expect(
    page.locator('.admin-toolbar').getByRole('button', { name: 'Announcements', exact: true }),
  ).toHaveCount(0);
  await page.goto('/news/990900');
  await expect(page.locator('main')).toContainText('This announcement is unavailable.');
  await expect(page.locator('.announcement-article')).toHaveCount(0);
});

test('public retry, empty states and normal errors use localized copy', async ({ page }) => {
  const state = await fixture(page, 'en');
  state.publicError = 503;
  await page.goto('/news');
  await expect(page.locator('main [role=alert]')).toContainText(
    "The server couldn't complete your request.",
  );
  await expect(page.locator('main')).not.toContainText('Raw untranslated fixture error');
  state.publicError = 0;
  await page.getByRole('button', { name: 'Try again', exact: true }).click();
  await expect(page.locator('.announcement-rows > li')).toHaveCount(12);
  state.items = [];
  await page.reload();
  await expect(
    page.getByRole('heading', { name: 'No announcements yet', exact: true }),
  ).toBeVisible();
  for (const [status, message] of [
    [401, 'Your session has expired.'],
    [403, 'You do not have permission'],
    [404, 'This announcement is unavailable.'],
  ] as const) {
    state.publicError = status;
    await page.goto(`/news/${990900 + status}`);
    await expect(page.locator('main [role=alert]')).toContainText(message);
  }
});

test('management read failures and vanished conflict targets retain recoverable text', async ({
  page,
}) => {
  const state = await fixture(page);
  state.adminError = 503;
  await page.goto('/admin?tab=announcements');
  await expect(page.locator('.announcement-management-list [role=alert]')).toContainText(
    "The server couldn't complete your request.",
  );
  state.adminError = 0;
  await page.getByRole('button', { name: 'Try again', exact: true }).click();
  await page.locator('.announcement-select').first().click();
  await page.getByLabel('Body', { exact: true }).fill('Retain on missing latest target.');
  state.conflict = true;
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect(page.locator('.announcement-conflict')).toBeVisible();
  state.items = state.items.filter((item) => item.id !== 990900);
  await page
    .locator('.announcement-conflict')
    .getByRole('button', { name: 'Reload latest version', exact: true })
    .click();
  await page
    .getByRole('group', { name: 'Replace your text with the latest version?' })
    .getByRole('button', { name: 'Reload latest version', exact: true })
    .click();
  await expect(page.locator('.announcement-editor .form-error')).toHaveText(
    'This announcement is unavailable.',
  );
  await expect(page.getByLabel('Body', { exact: true })).toHaveValue(
    'Retain on missing latest target.',
  );
});

test('withdrawing the final filtered row returns to the remaining last page', async ({ page }) => {
  const state = await fixture(page);
  await page.goto('/admin?tab=announcements&status=published&page=3');
  await expect(page.locator('.announcement-admin-rows > li')).toHaveCount(1);
  await page.locator('.announcement-select').first().click();
  await page.getByRole('button', { name: 'Withdraw', exact: true }).click();
  await page
    .getByRole('group', { name: 'Withdraw this announcement?' })
    .getByRole('button', { name: 'Withdraw', exact: true })
    .click();
  await expect(page).toHaveURL(/page=2/);
  await expect(page.locator('.announcement-admin-rows > li')).toHaveCount(12);
  await expect(page.locator('.announcement-editor')).toBeVisible();
  expect(state.writes).toHaveLength(1);
});

function disposableCredentials(): Record<string, string> {
  const envPath = process.env.ILLYA_TEST_ENV_FILE;
  const origin = process.env.ILLYA_WEB_URL ?? 'http://localhost:3000';
  if (
    process.env.ILLYA_ALLOW_DISPOSABLE_TEST !== '1' ||
    !envPath ||
    !path.isAbsolute(envPath) ||
    path.basename(envPath) === '.env' ||
    !/^http:\/\/(localhost|127\.0\.0\.1):[1-9][0-9]{0,4}$/.test(origin)
  )
    throw new Error(
      'Run the real announcement lifecycle only with infra/disposable-web-smoke.mjs.',
    );
  return Object.fromEntries(
    readFileSync(envPath, 'utf8')
      .split(/\r?\n/)
      .filter((line) => /^[A-Z_][A-Z0-9_]*=/.test(line))
      .map((line) => {
        const at = line.indexOf('=');
        return [line.slice(0, at), line.slice(at + 1)];
      }),
  );
}

test('owned API lifecycle enforces delegated grants, public visibility and revision conflicts', async () => {
  test.skip(
    process.env.ILLYA_ALLOW_DISPOSABLE_TEST !== '1',
    'Requires the explicitly owned disposable stack.',
  );
  const env = disposableCredentials();
  const baseURL = process.env.ILLYA_WEB_URL ?? 'http://localhost:3000';
  const contexts = await Promise.all(
    [0, 1, 2].map(() => apiRequest.newContext({ baseURL, extraHTTPHeaders: { Origin: baseURL } })),
  );
  const [operator, manager, visitor] = contexts;
  const suffix = `${Date.now().toString(36)}a`;
  let roleId: number | undefined;
  let userId: number | undefined;
  let announcementId: number | undefined;
  try {
    expect(
      (
        await operator.post('/api/v1/auth/login', {
          data: { username: env.BOOTSTRAP_ADMIN_USERNAME, password: env.BOOTSTRAP_ADMIN_PASSWORD },
        })
      ).ok(),
    ).toBe(true);
    const registered = await manager.post('/api/v1/auth/register', {
      data: {
        username: `ann_${suffix}`,
        email: `ann_${suffix}@example.test`,
        password: `Illya-Announcement-${suffix}-secure`,
        locale: 'en',
      },
    });
    expect(registered.ok()).toBe(true);
    userId = (await registered.json()).user.id;
    expect((await manager.get('/api/v1/admin/announcements')).status()).toBe(403);
    expect((await visitor.get('/api/v1/admin/announcements')).status()).toBe(401);
    const role = await operator.post('/api/v1/admin/roles', {
      data: {
        name: `announcements_${suffix}`,
        description: 'Disposable announcement lifecycle',
        permissions: ['admin.view', 'announcements.manage'],
      },
    });
    expect(role.ok()).toBe(true);
    roleId = (await role.json()).id;
    expect(
      (await operator.patch(`/api/v1/admin/users/${userId}`, { data: { roles: [roleId] } })).ok(),
    ).toBe(true);
    const created = await manager.post('/api/v1/admin/announcements', {
      data: {
        title: `Announcement lifecycle ${suffix}`,
        body: 'Plain text <b>stays literal</b>\r\nSecond line',
      },
    });
    expect(created.status()).toBe(201);
    const draft = await created.json();
    announcementId = draft.id;
    expect(draft.status).toBe('draft');
    expect(draft.body).toBe('Plain text <b>stays literal</b>\nSecond line');
    expect((await operator.get(`/api/v1/announcements/${announcementId}`)).status()).toBe(404);
    expect((await visitor.get(`/api/v1/announcements/${announcementId}`)).status()).toBe(404);
    expect((await manager.get(`/api/v1/admin/announcements/${announcementId}`)).status()).toBe(200);
    const publishedResponse = await manager.patch(`/api/v1/admin/announcements/${announcementId}`, {
      data: { title: draft.title, body: draft.body, status: 'published', revision: draft.revision },
    });
    expect(publishedResponse.ok()).toBe(true);
    const published = await publishedResponse.json();
    expect(published.published_at).not.toBeNull();
    expect((await visitor.get(`/api/v1/announcements/${announcementId}`)).status()).toBe(200);
    const publicList = await (
      await visitor.get('/api/v1/announcements?page=1&page_size=100')
    ).json();
    expect(publicList.items.some((item: Announcement) => item.id === announcementId)).toBe(true);
    const conflict = await manager.patch(`/api/v1/admin/announcements/${announcementId}`, {
      data: { title: draft.title, body: 'Stale edit', status: 'draft', revision: draft.revision },
    });
    expect(conflict.status()).toBe(409);
    expect((await conflict.json()).error.code).toBe('announcement_conflict');
    const withdrawn = await manager.patch(`/api/v1/admin/announcements/${announcementId}`, {
      data: {
        title: published.title,
        body: published.body,
        status: 'archived',
        revision: published.revision,
      },
    });
    expect(withdrawn.ok()).toBe(true);
    expect((await visitor.get(`/api/v1/announcements/${announcementId}`)).status()).toBe(404);
    expect((await operator.get(`/api/v1/announcements/${announcementId}`)).status()).toBe(404);
    expect(
      (await operator.patch(`/api/v1/admin/users/${userId}`, { data: { roles: [] } })).ok(),
    ).toBe(true);
    expect((await manager.get(`/api/v1/admin/announcements/${announcementId}`)).status()).toBe(403);
  } finally {
    if (announcementId) {
      const retained = await operator.get(`/api/v1/admin/announcements/${announcementId}`);
      if (retained.ok()) {
        const current = await retained.json();
        await operator.patch(`/api/v1/admin/announcements/${announcementId}`, {
          data: {
            title: current.title,
            body: current.body,
            status: 'archived',
            revision: current.revision,
          },
        });
      }
    }
    if (userId) await operator.patch(`/api/v1/admin/users/${userId}`, { data: { roles: [] } });
    if (roleId) await operator.delete(`/api/v1/admin/roles/${roleId}`);
    await Promise.all(contexts.map((context) => context.dispose()));
  }
});
