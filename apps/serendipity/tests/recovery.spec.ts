import { selectField } from './helpers/field-select';
import { test, expect, type Page } from '@playwright/test';

// Browser-only fixtures: no mail is sent and no real community account is changed.
const token = 'ab'.repeat(32);
const password = 'Illyaverse-Recovery-Fixture-Only';

async function guest(page: Page) {
  await page.route('**/api/v1/me', (route) =>
    route.fulfill({
      status: 401,
      json: { error: { code: 'unauthorized', message: 'Sign in required' } },
    }),
  );
}

async function capture(page: Page, name: string) {
  if (process.env.ILLYA_CAPTURE_RECOVERY_REVIEW === '1')
    await page.screenshot({ path: `../../data-recovery-review/${name}.png`, fullPage: true });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
}

async function emailCapabilities(page: Page, available = true) {
  await page.route('**/api/v1/auth/capabilities', (route) =>
    route.fulfill({
      json: { password_reset: available, email_verification: available },
    }),
  );
}

test('forgot password has an honest unavailable state and a generic accepted response', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await guest(page);
  let available = false;
  let requests = 0;
  await page.route('**/api/v1/auth/capabilities', (route) =>
    route.fulfill({
      json: { password_reset: available, email_verification: available },
    }),
  );
  await page.route('**/api/v1/auth/recovery/request', (route) => {
    requests++;
    expect(route.request().postDataJSON()).toEqual({ email: 'fixture@example.test', locale: 'en' });
    return route.fulfill({ status: 202, json: { message: 'Request accepted' } });
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/login');
  await page.getByRole('link', { name: '忘記密碼？', exact: true }).click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('重設密碼');
  await expect(page.getByRole('button', { name: '取得重設連結', exact: true })).toBeDisabled();
  await expect(
    page.getByText('郵件服務暫時無法使用。請稍後再試，或聯絡社群管理員。'),
  ).toBeVisible();
  expect(requests).toBe(0);
  await capture(page, 'forgot-unavailable-desktop-zh-TW');

  available = true;
  await page.setViewportSize({ width: 390, height: 844 });
  await selectField(page.locator('.language-select'), 'en');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Reset your password');
  await page.reload();
  await page.getByLabel('Email', { exact: true }).fill('fixture@example.test');
  await page.getByRole('button', { name: 'Request reset link', exact: true }).click();
  await expect(page.getByText('Your request has been received.', { exact: true })).toBeVisible();
  await expect(page.getByText(/If an eligible account uses this email/)).toBeVisible();
  expect(requests).toBe(1);
  await capture(page, 'forgot-accepted-mobile-en');
  expect(errors).toEqual([]);
});

test('reset clears its fragment and sends a token only after matching passwords are submitted', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await guest(page);
  let requests = 0;
  await page.route('**/api/v1/auth/recovery/reset', (route) => {
    requests++;
    expect(route.request().url()).not.toContain(token);
    expect(route.request().postDataJSON()).toEqual({ token, password });
    return route.fulfill({ json: { message: 'Password reset' } });
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`/reset-password#token=${token}`);
  await expect(page.getByLabel('新密碼', { exact: true })).toBeVisible();
  await expect(page).toHaveURL(/\/reset-password$/);
  expect(requests).toBe(0);
  expect(
    await page.evaluate(
      (secret) =>
        [...Object.values(localStorage), ...Object.values(sessionStorage)].some((value) =>
          String(value).includes(secret),
        ),
      token,
    ),
  ).toBe(false);
  await page.getByLabel('新密碼', { exact: true }).fill(password);
  await page.getByLabel('再次輸入新密碼', { exact: true }).fill('Different-Fixture-Password');
  await page.getByRole('button', { name: '重設密碼', exact: true }).click();
  await expect(page.getByText('兩次輸入的密碼不同，請確認後重試。')).toBeVisible();
  expect(requests).toBe(0);
  await capture(page, 'reset-mismatch-desktop-zh-TW');

  await page.getByLabel('再次輸入新密碼', { exact: true }).fill(password);
  await page.getByRole('button', { name: '重設密碼', exact: true }).click();
  await expect(page.getByText('密碼已更新。', { exact: true })).toBeVisible();
  await expect(
    page.getByText('所有裝置的登入已失效。請用新密碼重新登入網站與遊戲客戶端。'),
  ).toBeVisible();
  expect(requests).toBe(1);
  await page.setViewportSize({ width: 390, height: 844 });
  await capture(page, 'reset-success-mobile-zh-TW');
  await page.reload();
  await expect(page.locator('.form-error[role="alert"]')).toContainText(
    '此連結已失效、已使用，或不完整。',
  );
  await expect(page.getByRole('link', { name: '重新取得重設連結' })).toHaveAttribute(
    'href',
    '/forgot-password',
  );
  expect(requests).toBe(1);
  expect(errors).toEqual([]);
});

test('verification requires an intentional action and explains a used or invalid link', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await guest(page);
  let requests = 0;
  let valid = false;
  const nextToken = 'cd'.repeat(32);
  const newestToken = 'ef'.repeat(32);
  let releaseOldResponse: (() => void) | undefined;
  await page.route('**/api/v1/auth/email/verify', async (route) => {
    requests++;
    const submitted = route.request().postDataJSON().token;
    expect([token, nextToken, newestToken]).toContain(submitted);
    if (submitted === nextToken) {
      await new Promise<void>((resolve) => {
        releaseOldResponse = resolve;
      });
      return route.fulfill({
        status: 400,
        json: { error: { code: 'invalid_recovery_token', message: 'Invalid token' } },
      });
    }
    return valid
      ? route.fulfill({ json: { message: 'Email verified' } })
      : route.fulfill({
          status: 400,
          json: { error: { code: 'invalid_recovery_token', message: 'Invalid token' } },
        });
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`/verify-email#token=${token}`);
  await expect(page.getByRole('button', { name: '確認電子郵件', exact: true })).toBeVisible();
  await expect(page).toHaveURL(/\/verify-email$/);
  expect(requests).toBe(0);
  await capture(page, 'verify-ready-desktop-zh-TW');
  await page.getByRole('button', { name: '確認電子郵件', exact: true }).click();
  await expect(page.locator('.form-error[role="alert"]')).toContainText(
    '此連結已失效、已使用，或不完整。',
  );
  await expect(page.getByText('登入後，可在帳號設定重新申請驗證郵件。')).toBeVisible();
  expect(requests).toBe(1);

  valid = true;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/verify-email#token=${token}`);
  await page.getByRole('button', { name: '確認電子郵件', exact: true }).click();
  await expect(page.getByText('電子郵件已驗證。', { exact: true })).toBeVisible();
  await capture(page, 'verify-success-mobile-zh-TW');
  expect(requests).toBe(2);

  // The browser can open a second email in the same tab without remounting this
  // route. A late response for that link must not overwrite the next link's state.
  await page.goto(`/verify-email#token=${nextToken}`);
  await expect(page).toHaveURL(/\/verify-email$/);
  const confirm = page.getByRole('button', { name: '確認電子郵件', exact: true });
  await expect(confirm).toBeEnabled();
  await expect(page.getByText('電子郵件已驗證。', { exact: true })).toHaveCount(0);
  expect(requests).toBe(2);
  await confirm.click();
  await expect.poll(() => requests).toBe(3);
  await expect(page.getByRole('button', { name: '處理中…', exact: true })).toBeDisabled();

  await page.goto(`/verify-email#token=${newestToken}`);
  await expect(page).toHaveURL(/\/verify-email$/);
  await expect(confirm).toBeEnabled();
  const oldResponse = page.waitForResponse(
    (response) =>
      response.url().endsWith('/api/v1/auth/email/verify') &&
      response.request().postDataJSON().token === nextToken,
  );
  releaseOldResponse!();
  await (await oldResponse).finished();
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())));
  await expect(confirm).toBeEnabled();
  await expect(page.locator('.form-error[role="alert"]')).toHaveCount(0);
  expect(requests).toBe(3);
  await confirm.click();
  await expect(page.getByText('電子郵件已驗證。', { exact: true })).toBeVisible();
  expect(requests).toBe(4);
  expect(errors).toEqual([]);
});

test('account settings requests verification and shows its persisted verified state in Japanese', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  const fixture = {
    id: 987600,
    username: 'Recovery fixture',
    email: 'fixture@example.test',
    email_verified: false,
    country: 'TW',
    bio: '',
    avatar_url: null,
    banner_url: null,
    accent_color: '#f28eb6',
    roles: ['player'],
    permissions: [],
    joined_at: '2026-10-07T00:00:00Z',
    pp: 0,
    play_count: 0,
  };
  await page.route('**/api/v1/me', (route) => route.fulfill({ json: fixture }));
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
  await emailCapabilities(page);
  let requests = 0;
  await page.route('**/api/v1/auth/email/request', (route) => {
    requests++;
    expect(route.request().method()).toBe('POST');
    return route.fulfill({ status: 202, json: { message: 'Request accepted' } });
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/settings');
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
  await page.getByRole('tab', { name: 'アカウント', exact: true }).click();
  await expect(
    page.getByText('このアドレスは未認証です。メールを受信できることをご確認ください。'),
  ).toBeVisible();
  await page.getByRole('button', { name: '認証メールを申請', exact: true }).click();
  await expect(page.getByText('認証の申請を受け付けました。')).toBeVisible();
  await expect(page.getByRole('button', { name: '認証メールを申請', exact: true })).toBeDisabled();
  expect(requests).toBe(1);
  await capture(page, 'settings-requested-desktop-ja');

  fixture.email_verified = true;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload();
  await page.getByRole('tab', { name: 'アカウント', exact: true }).click();
  await expect(page.getByText('認証済み', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '認証メールを申請', exact: true })).toHaveCount(0);
  await capture(page, 'settings-verified-mobile-ja');
  expect(errors).toEqual([]);
});
