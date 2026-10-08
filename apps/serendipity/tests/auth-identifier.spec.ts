import { test, expect, type Page } from '@playwright/test';
import { randomBytes } from 'node:crypto';
import { readFileSync, realpathSync, statSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

// Five UI cases intercept auth requests. The final case requires the helper's owned disposable stack.
const boundaryEmail = `${'a'.repeat(64)}@${'b'.repeat(63)}.${'c'.repeat(63)}.${'d'.repeat(56)}.test`;
const fixturePassword = 'Auth-Identifier-Fixture-Only';

async function guest(
  page: Page,
  baseURL: string | undefined,
  locale: 'en' | 'zh-TW' | 'ja',
  mode: 'dark' | 'light' = 'dark',
) {
  await page.context().addCookies([
    { name: 'illya_locale', value: locale, url: baseURL ?? 'http://localhost:3000' },
  ]);
  await page.addInitScript(
    ({ locale, mode }) => {
      localStorage.setItem('theme', mode);
      localStorage.setItem(
        'illya_appearance',
        JSON.stringify({
          locale,
          mode,
          accent_color: '#f28eb6',
          background_url: null,
          background_blur: 0,
          background_dim: 0.76,
          panel_blur: 8,
          panel_opacity: 0.55,
          border_radius: 14,
        }),
      );
    },
    { locale, mode },
  );
  await page.route('**/api/v1/**', (route) =>
    route.fulfill({
      status: new URL(route.request().url()).pathname === '/api/v1/me' ? 401 : 404,
      json: { error: { code: 'fixture_guest' } },
    }),
  );
}

async function noPageOverflow(page: Page) {
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
}

async function capture(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready);
  await noPageOverflow(page);
  if (process.env.ILLYA_CAPTURE_WORKFLOW_REVIEW === '1') {
    await page.evaluate(() => {
      (document.activeElement as HTMLElement | null)?.blur();
      window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
    });
    await page.mouse.move(0, 0);
    await page.screenshot({
      path: `../../.impeccable/review/auth-identifier-${name}.png`,
      fullPage: true,
      animations: 'disabled',
    });
  }
}

test('login submits a complete 254-character email and recovers from credential errors', async ({
  page,
  baseURL,
}) => {
  await guest(page, baseURL, 'en');
  await page.setViewportSize({ width: 1440, height: 1000 });
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  const usernames: string[] = [];
  let releaseResponse: (() => void) | undefined;
  await page.route('**/api/v1/auth/login', async (route) => {
    expect(route.request().method()).toBe('POST');
    const body = route.request().postDataJSON() as Record<string, unknown>;
    expect(Object.keys(body).sort()).toEqual(['password', 'username']);
    // Compare as a boolean so assertion output never prints the password payload.
    expect(body.password === fixturePassword).toBe(true);
    usernames.push(String(body.username));
    await new Promise<void>((resolve) => {
      releaseResponse = resolve;
    });
    await route.fulfill({
      status: 401,
      json: {
        error: {
          code: 'invalid_credentials',
          message: usernames.length === 1 ? 'Unknown fixture account' : 'Wrong fixture password',
        },
      },
    });
  });
  await page.goto('/login');
  const identifier = page.getByLabel('Username or email', { exact: true });
  const password = page.getByLabel('Password', { exact: true });
  const submit = page.getByRole('button', { name: 'Sign in', exact: true });
  await expect(identifier).toHaveAttribute('name', 'username');
  await expect(identifier).toHaveAttribute('autocomplete', 'username');
  await expect(identifier).toHaveAttribute('maxlength', '254');
  await expect(password).toHaveAttribute('autocomplete', 'current-password');
  await expect(password).toHaveAttribute('type', 'password');
  expect(boundaryEmail.length).toBe(254);
  await identifier.fill(boundaryEmail);
  await expect(identifier).toHaveValue(boundaryEmail);
  await password.fill(fixturePassword);
  await password.press('Enter');
  await expect.poll(() => usernames.length).toBe(1);
  expect(usernames[0]).toBe(boundaryEmail);
  await expect(page.getByRole('button', { name: 'Submitting…', exact: true })).toBeDisabled();
  releaseResponse!();
  const alert = page.locator('form').getByRole('alert');
  await expect(alert).toHaveText('Check your details and try again.');
  await expect(submit).toBeEnabled();
  await expect(identifier).toHaveValue(boundaryEmail);
  expect((await password.inputValue()) === fixturePassword).toBe(true);
  await capture(page, 'desktop-dark-en-login-error');

  await identifier.fill('KnownFixtureUsername');
  await password.press('Enter');
  await expect.poll(() => usernames.length).toBe(2);
  expect(usernames[1]).toBe('KnownFixtureUsername');
  await expect(alert).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Submitting…', exact: true })).toBeDisabled();
  releaseResponse!();
  await expect(alert).toHaveText('Check your details and try again.');
  await expect(submit).toBeEnabled();
  await expect(identifier).toHaveValue('KnownFixtureUsername');
  expect((await password.inputValue()) === fixturePassword).toBe(true);
  await expect(page).toHaveURL(/\/login$/);
  expect(errors).toEqual([]);
});

test('registration retains the 24-character username boundary and separate email field', async ({
  page,
  baseURL,
}) => {
  await guest(page, baseURL, 'ja', 'light');
  await page.setViewportSize({ width: 390, height: 844 });
  let requests = 0;
  await page.route('**/api/v1/auth/register', (route) => {
    requests++;
    const body = route.request().postDataJSON() as Record<string, unknown>;
    expect(Object.keys(body).sort()).toEqual(['email', 'locale', 'password', 'username']);
    expect(body.username).toBe('r'.repeat(24));
    expect(body.email).toBe(boundaryEmail);
    expect(body.locale).toBe('ja');
    expect(body.password === fixturePassword).toBe(true);
    return route.fulfill({
      status: 409,
      json: { error: { code: 'fixture_account_exists' } },
    });
  });
  await page.goto('/register');
  const username = page.getByLabel('ユーザー名', { exact: true });
  const email = page.getByLabel('メールアドレス', { exact: true });
  const password = page.getByLabel('パスワード', { exact: true });
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
  await expect(page.locator('html')).toHaveClass(/light/);
  await expect(username).toHaveAttribute('name', 'username');
  await expect(username).toHaveAttribute('autocomplete', 'username');
  await expect(username).toHaveAttribute('minlength', '3');
  await expect(username).toHaveAttribute('maxlength', '24');
  await expect(email).toHaveAttribute('type', 'email');
  await expect(email).toHaveAttribute('autocomplete', 'email');
  await expect(email).toHaveAttribute('maxlength', '254');
  await expect(password).toHaveAttribute('autocomplete', 'new-password');
  await expect(password).toHaveAttribute('minlength', '12');
  await expect(password).toHaveAttribute('maxlength', '128');
  await username.fill('r'.repeat(25));
  await expect(username).toHaveValue('r'.repeat(24));
  await email.fill(boundaryEmail);
  await expect(email).toHaveValue(boundaryEmail);
  await password.fill(fixturePassword);
  await capture(page, 'mobile-light-ja-register-boundary');
  await password.press('Enter');
  await expect(page.locator('form').getByRole('alert')).toHaveText('入力内容をご確認ください。');
  expect(requests).toBe(1);
  await expect(username).toHaveValue('r'.repeat(24));
  await expect(page.getByRole('button', { name: 'アカウント作成', exact: true })).toBeEnabled();
  await expect(page).toHaveURL(/\/register$/);
});

for (const [locale, loginLabel, registrationLabel] of [
  ['en', 'Username or email', 'Username'],
  ['zh-TW', '使用者名稱或電子郵件', '使用者名稱'],
  ['ja', 'ユーザー名またはメールアドレス', 'ユーザー名'],
] as const) {
  test(`${locale} distinguishes login identifiers from registration usernames on mobile`, async ({
    page,
    baseURL,
  }) => {
    await guest(page, baseURL, locale);
    await page.setViewportSize({ width: 320, height: 844 });
    await page.goto('/login');
    await expect(page.getByLabel(loginLabel, { exact: true })).toBeVisible();
    await expect(page.locator('#email')).toHaveCount(0);
    await noPageOverflow(page);
    await page.goto('/register');
    await expect(page.getByLabel(registrationLabel, { exact: true })).toBeVisible();
    await expect(page.getByLabel(loginLabel, { exact: true })).toHaveCount(0);
    await noPageOverflow(page);
  });
}

function ownedDisposableOrigin(baseURL: string | undefined) {
  const origin = 'http://localhost:19000';
  const envFile = process.env.ILLYA_TEST_ENV_FILE;
  const samePath = (left: string, right: string) =>
    process.platform === 'win32' ? left.toLowerCase() === right.toLowerCase() : left === right;
  try {
    if (
      process.env.ILLYA_ALLOW_DISPOSABLE_TEST !== '1' ||
      baseURL !== origin ||
      process.env.ILLYA_WEB_URL !== origin ||
      !/^illya-web-smoke-[a-f0-9]{16}$/.test(process.env.COMPOSE_PROJECT_NAME ?? '') ||
      !envFile ||
      !path.isAbsolute(envFile) ||
      path.basename(envFile) !== 'test.env'
    )
      throw new Error();
    const resolvedFile = realpathSync(envFile);
    const tempDirectory = path.dirname(resolvedFile);
    if (
      !samePath(resolvedFile, path.resolve(envFile)) ||
      !samePath(path.dirname(tempDirectory), realpathSync(os.tmpdir())) ||
      !/^illya-web-smoke-[A-Za-z0-9]{6}$/.test(path.basename(tempDirectory)) ||
      !statSync(resolvedFile).isFile()
    )
      throw new Error();
    const values: Record<string, string> = Object.fromEntries(
      readFileSync(resolvedFile, 'utf8')
        .split(/\r?\n/)
        .filter((line) => /^[A-Z_][A-Z0-9_]*=/.test(line))
        .map((line) => {
          const at = line.indexOf('=');
          return [line.slice(0, at), line.slice(at + 1)];
        }),
    );
    // Match this invocation's generated markers without displaying environment secrets.
    if (
      values.ILLYA_WEB_SMOKE_PORT !== '19000' ||
      process.env.ILLYA_WEB_SMOKE_PORT !== '19000' ||
      !/^web_smoke_admin_[a-f0-9]{8}$/.test(values.BOOTSTRAP_ADMIN_USERNAME ?? '') ||
      values.BOOTSTRAP_ADMIN_USERNAME !== process.env.BOOTSTRAP_ADMIN_USERNAME ||
      ['POSTGRES_PASSWORD', 'SESSION_SECRET', 'INTERNAL_SERVICE_KEY'].some(
        (key) => !/^[a-f0-9]{64}$/.test(values[key] ?? '') || values[key] !== process.env[key],
      ) ||
      ['POSTGRES', 'SANCTUARY', 'RESONANCE', 'SERENDIPITY', 'CADDY'].some((name) => {
        const key = `ILLYA_WEB_SMOKE_${name}_IMAGE`;
        return !/^sha256:[a-f0-9]{64}$/.test(values[key] ?? '') || values[key] !== process.env[key];
      })
    )
      throw new Error();
    return origin;
  } catch {
    throw new Error(
      'Real auth requires infra/disposable-web-smoke.mjs with its current owned temporary fixture at localhost:19000.',
    );
  }
}

test('disposable website registration logs out and signs back in by its full email', async ({
  page,
  baseURL,
}) => {
  test.skip(
    process.env.ILLYA_ALLOW_DISPOSABLE_TEST !== '1',
    'Real account creation requires the disposable website smoke helper.',
  );
  const origin = ownedDisposableOrigin(baseURL);
  const suffix = randomBytes(6).toString('hex');
  const username = `authmail_${suffix}`;
  const email = `long.auth.identifier.${suffix}@example.test`;
  const password = `Auth-mail-fixture-${randomBytes(18).toString('hex')}`;
  expect(username.length).toBeLessThan(24);
  expect(email.length).toBeGreaterThan(24);
  expect(email.split('@')[0].length).toBeLessThan(64);
  await page.context().addCookies([{ name: 'illya_locale', value: 'en', url: origin }]);
  await page.goto('/register');
  await page.getByLabel('Username', { exact: true }).fill(username);
  await page.getByLabel('Email', { exact: true }).fill(email);
  await page.getByLabel('Password', { exact: true }).fill(password);
  const registration = page.waitForResponse(
    (response) =>
      response.url() === `${origin}/api/v1/auth/register` && response.request().method() === 'POST',
  );
  await page.getByRole('button', { name: 'Create account', exact: true }).click();
  const registered = await registration;
  expect(registered.ok()).toBe(true);
  const playerId = (await registered.json()).user.id as number;
  expect(Number.isSafeInteger(playerId) && playerId > 0).toBe(true);
  await expect(page).toHaveURL(`${origin}/connect`);
  await expect(page.getByRole('link', { name: 'My profile', exact: true })).toHaveAttribute(
    'href',
    `/players/${playerId}`,
  );

  const logout = page.waitForResponse(
    (response) =>
      response.url() === `${origin}/api/v1/auth/logout` && response.request().method() === 'POST',
  );
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  expect((await logout).ok()).toBe(true);
  await expect(page).toHaveURL(`${origin}/`);
  expect((await page.request.get(`${origin}/api/v1/me`)).status()).toBe(401);

  await page.goto('/login');
  const identifier = page.getByLabel('Username or email', { exact: true });
  await identifier.fill(email);
  await expect(identifier).toHaveValue(email);
  const passwordField = page.getByLabel('Password', { exact: true });
  await passwordField.fill(password);
  const login = page.waitForResponse(
    (response) =>
      response.url() === `${origin}/api/v1/auth/login` && response.request().method() === 'POST',
  );
  await passwordField.press('Enter');
  const signedIn = await login;
  expect(signedIn.request().postDataJSON().username).toBe(email);
  expect(signedIn.request().postDataJSON().password === password).toBe(true);
  expect(signedIn.ok()).toBe(true);
  expect((await signedIn.json()).user.id).toBe(playerId);
  await expect(page).toHaveURL(`${origin}/players/${playerId}`);
  await expect(page.locator('.profile-identity h1')).toHaveText(username);
  const me = await page.request.get(`${origin}/api/v1/me`);
  expect(me.ok()).toBe(true);
  expect((await me.json()).id).toBe(playerId);
});
