import { selectField } from './helpers/field-select';
import { test, expect, request as apiRequest } from '@playwright/test';
import { readFileSync } from 'node:fs';
import path from 'node:path';
const suffix = Date.now().toString(36);
const username = `e2e_${suffix}`;
const password = `Illyaverse-E2E-${suffix}-secure`;
const webOrigin = process.env.ILLYA_WEB_URL ?? 'http://localhost:3000';

// These two tests write real accounts. Only an explicitly owned test stack may run them.
function disposableCredentials(): Record<string, string> {
  const envPath = process.env.ILLYA_TEST_ENV_FILE;
  if (
    process.env.ILLYA_ALLOW_DISPOSABLE_TEST !== '1' ||
    !envPath ||
    !path.isAbsolute(envPath) ||
    path.basename(envPath) === '.env' ||
    !/^http:\/\/(localhost|127\.0\.0\.1):[1-9][0-9]{0,4}$/.test(webOrigin)
  )
    throw new Error('Run real community workflows with infra/disposable-web-smoke.mjs.');
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
test('public pages, pure black default, multilingual cookie and mobile navigation', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Illyaverse');
  await expect
    .poll(() => page.evaluate(() => getComputedStyle(document.body).backgroundColor))
    .toBe('rgb(0, 0, 0)');
  await expect(page.locator('.global-scene')).toHaveCSS('background-image', 'none');
  await selectField(page.locator('.language-select'), 'en');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Illyaverse');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await page.goto('/docs/connection');
  await expect(page.locator('.markdown-document h1')).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open navigation', exact: true }).click();
  await page.locator('.mobile-nav').getByRole('link', { name: 'Beatmaps', exact: true }).click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Beatmaps');
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
  ).toBeTruthy();
});
test('registration, persistent locale/theme/profile, chat draft and server RBAC', async ({
  page,
  request,
}) => {
  disposableCredentials();
  await page.goto('/register');
  await page.getByLabel('使用者名稱', { exact: true }).fill(username);
  await page.getByLabel('電子郵件', { exact: true }).fill(`${username}@example.test`);
  await page.getByLabel('密碼', { exact: true }).fill(password);
  await page.getByRole('button', { name: '建立帳號', exact: true }).click();
  await expect(page).toHaveURL(/\/connect$/);
  await page.goto('/settings');
  await page
    .getByRole('button', { name: 'Light', exact: true })
    .count()
    .then(async (count) => {
      if (count) await page.getByRole('button', { name: 'Light', exact: true }).click();
      else await page.getByRole('button', { name: '淺色', exact: true }).click();
    });
  await page.locator('#accent').fill('#7dd8c5');
  await page.getByRole('button', { name: '儲存變更', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '已儲存變更' })).toBeVisible();
  const preferences = await (await page.request.get('/api/v1/me/preferences')).json();
  expect(preferences.mode).toBe('light');
  expect(preferences.accent_color).toBe('#7dd8c5');
  const fixture = await page.evaluate(() => {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 2;
    const drawing = canvas.getContext('2d')!;
    drawing.fillStyle = '#7dd8c5';
    drawing.fillRect(0, 0, 2, 2);
    return canvas.toDataURL('image/png').split(',')[1];
  });
  await page.locator('.appearance-controls input[type=file]').setInputFiles({
    name: 'background-fixture.png',
    mimeType: 'image/png',
    buffer: Buffer.from(fixture, 'base64'),
  });
  await expect(page.locator('.global-scene')).not.toHaveCSS('background-image', 'none');
  await page.getByRole('tab', { name: '帳號', exact: true }).click();
  await selectField(page.locator('#account-locale'), 'ja');
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
  await expect(page.locator('.global-scene')).not.toHaveCSS('background-image', 'none');
  await page.reload();
  await expect(page.locator('html')).toHaveClass(/light/);
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
  await page.goto('/settings');
  await page.getByRole('tab', { name: 'プロフィール', exact: true }).click();
  await page
    .getByLabel('自己紹介', { exact: true })
    .fill('E2E test profile — original local fixture');
  await page.getByRole('button', { name: '変更を保存', exact: true }).click();
  await expect(page.locator('.settings-form .form-success')).toContainText('保存しました');
  await page.goto('/admin');
  await expect(
    page.getByRole('heading', { name: 'このエリアへのアクセス権がありません。' }),
  ).toBeVisible();
  const protectedResponse = await page.request.get('/api/v1/admin/users');
  expect(protectedResponse.status()).toBe(403);
  await page.goto('/chat');
  await page.getByLabel('メッセージを書く…', { exact: true }).fill(`E2E message ${suffix}`);
  await page.getByRole('button', { name: '送信', exact: true }).click();
  await expect(
    page.locator('.message p').filter({ hasText: `E2E message ${suffix}` }),
  ).toBeVisible();
  const me = await page.request.get('/api/v1/me');
  expect(me.ok()).toBeTruthy();
  const user = await me.json();
  await page.goto(`/players/${user.id}`);
  await expect(page.locator('.profile-bio')).toHaveText(
    'E2E test profile — original local fixture',
  );
  const peerContext = await apiRequest.newContext({
    baseURL: webOrigin,
    extraHTTPHeaders: { Origin: webOrigin },
  });
  const peerRegistration = await peerContext.post('/api/v1/auth/register', {
    data: { username: `peer_${suffix}`, email: `peer_${suffix}@example.test`, password },
  });
  expect(peerRegistration.ok()).toBeTruthy();
  const peer = await peerRegistration.json();
  await page.goto(`/players/${peer.user.id}`);
  await page.getByRole('button', { name: 'フレンドに追加', exact: true }).click();
  await expect(page.getByRole('button', { name: 'フレンドから削除', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'ブロック', exact: true }).click();
  await page.getByRole('button', { name: 'ブロックする', exact: true }).click();
  await expect(page.getByRole('button', { name: 'ブロック解除', exact: true })).toBeVisible();
  expect(
    (await page.request.get('/api/v1/me/relationships').then((r) => r.json())).friends,
  ).toEqual([]);
  await page.goto('/settings');
  await page.getByRole('tab', { name: 'フレンドとブロック', exact: true }).click();
  await expect(page.locator('.relationship-row')).toContainText(peer.user.username);
  await page.getByRole('button', { name: 'ブロック解除', exact: true }).click();
  await expect(page.locator('.relationship-row')).toHaveCount(0);
  await peerContext.dispose();
  await page.getByRole('button', { name: 'ログアウト', exact: true }).click();
  await expect(page).toHaveURL(/\/$/);
  expect((await page.request.get('/api/v1/me')).status()).toBe(401);
  expect((await request.get('/api/v1/admin/users')).status()).toBe(401);
});
test('operator can inspect permissions and audit without revealing secrets', async ({ page }) => {
  const env = disposableCredentials();
  if (!env.BOOTSTRAP_ADMIN_USERNAME || !env.BOOTSTRAP_ADMIN_PASSWORD)
    throw new Error('The disposable stack must provide its bootstrap account.');
  await page.goto('/login');
  await page.getByLabel('使用者名稱或電子郵件', { exact: true }).fill(env.BOOTSTRAP_ADMIN_USERNAME);
  await page.getByLabel('密碼', { exact: true }).fill(env.BOOTSTRAP_ADMIN_PASSWORD);
  await page.getByRole('button', { name: '登入', exact: true }).click();
  await expect(page).toHaveURL(/\/players\/\d+$/);
  await page.goto('/admin');
  await page.getByRole('button', { name: '角色與權限', exact: true }).click();
  await expect(page.locator('.role-row').filter({ hasText: 'administrator' })).toBeVisible();
  await page.getByRole('button', { name: '操作紀錄', exact: true }).click();
  const response = await page.request.get('/api/v1/admin/audit');
  expect(response.ok()).toBeTruthy();
  const audit = await response.json();
  await expect(page.locator(audit.total ? '.data-table' : '.empty-state')).toBeVisible();
});
