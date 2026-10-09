import { expect, test, type Page, type Route } from '@playwright/test';

// All community data is intercepted. Tests never modify an account or send a message.
const playerId = 989930;
const stamp = '2026-10-09T00:00:00Z';
const empty = { items: [], total: 0, page: 1, page_size: 24 };
function deferred() {
  let resolve = () => {};
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

async function fixture(page: Page, catalogue?: (route: Route, url: URL) => Promise<void>) {
  await page
    .context()
    .addCookies([{ name: 'illya_locale', value: 'en', url: test.info().project.use.baseURL! }]);
  await page.context().route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.replace('/api/v1', '');
    if (path === '/me')
      return route.fulfill({ status: 401, json: { error: { code: 'unauthorized' } } });
    if (path === '/beatmapsets' && catalogue) return catalogue(route, url);
    if (path === `/users/${playerId}`)
      return route.fulfill({
        json: {
          id: playerId,
          username: 'Navigation fixture',
          country: 'TW',
          bio: '',
          avatar_url: null,
          banner_url: null,
          accent_color: '#f28eb6',
          roles: [],
          permissions: [],
          joined_at: stamp,
          pp: 0,
          play_count: 0,
        },
      });
    if (path.endsWith('/rank-history'))
      return route.fulfill({
        json: {
          mode: 0,
          variant: 'vanilla',
          days: 90,
          start_date: '2026-07-12',
          end_date: '2026-10-09',
          time_zone: 'UTC',
          collection_interval_seconds: 1800,
          last_captured_at: null,
          points: [],
        },
      });
    if (path.endsWith('/scores/best'))
      return route.fulfill({
        json: {
          ...empty,
          total: 1,
          items: [
            {
              id: 989931,
              user_id: playerId,
              username: 'Navigation fixture',
              beatmap_id: 989932,
              title: 'Navigation score fixture',
              version: 'Extra',
              mode: 0,
              score_variant: 'vanilla',
              score: 12345,
              accuracy: 99.5,
              max_combo: 100,
              mods: 0,
              pp: 12.5,
              passed: true,
              verification: 'verified',
              created_at: stamp,
              has_replay: false,
              replay_complete: false,
              position: 1,
            },
          ],
        },
      });
    return route.fulfill({ json: empty });
  });
}

const progress = (page: Page) => page.locator('.navigation-progress');
async function shellGeometry(page: Page) {
  return page.locator('.header-inner').evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return { x: rect.x, width: rect.width, client: document.documentElement.clientWidth };
  });
}
async function expectStableShell(page: Page, initial: Awaited<ReturnType<typeof shellGeometry>>) {
  const current = await shellGeometry(page);
  expect(Math.abs(current.x - initial.x)).toBeLessThan(1);
  expect(Math.abs(current.width - initial.width)).toBeLessThan(1);
  expect(current.client).toBe(initial.client);
}
async function injectLateScrollLockCompensation(page: Page) {
  // Reproduce RemoveScroll's later rule even when headless Chromium uses overlay scrollbars.
  await page.addStyleTag({
    content: `body[data-scroll-locked] { overflow: hidden !important; position: relative !important; margin-right: 17px !important; }
      body[data-scroll-locked] { --removed-body-scroll-bar-size: 17px; }`,
  });
}

test('catalogue request progress follows query success, error and rapid cancellation', async ({
  page,
}, info) => {
  const first = deferred();
  const second = deferred();
  const cancelled = deferred();
  const requested: string[] = [];
  await fixture(page, async (route, url) => {
    const query = url.searchParams.get('q') ?? '';
    requested.push(query);
    const hold =
      query === 'first'
        ? first
        : query === 'second'
          ? second
          : query === 'cancelled'
            ? cancelled
            : null;
    if (hold) await hold.promise;
    if (query === 'error')
      await route.fulfill({ status: 503, json: { error: { code: 'service_unavailable' } } });
    else await route.fulfill({ json: empty }).catch(() => {}); // An older fetch may already be aborted.
  });
  await page.goto('/beatmaps');
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  const input = page.getByRole('textbox', { name: 'Search', exact: true });
  const submit = page
    .locator('.discovery-search')
    .getByRole('button', { name: 'Search', exact: true });
  await input.fill('first');
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  await submit.click();
  await expect.poll(() => requested.includes('first')).toBe(true);
  await expect(progress(page)).toHaveAttribute('data-pending', 'true');
  await expect(progress(page)).toHaveCSS('opacity', '1');
  await page.screenshot({ path: info.outputPath('navigation-pending-query.png') });
  await input.fill('second');
  await submit.click();
  await expect.poll(() => requested.includes('second')).toBe(true);
  first.resolve();
  await expect(progress(page)).toHaveAttribute('data-pending', 'true');
  second.resolve();
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  await expect(page).toHaveURL(/q=second/);

  await input.fill('error');
  await submit.click();
  await expect.poll(() => requested.includes('error')).toBe(true);
  await expect(
    page.getByRole('heading', { name: 'Beatmaps could not be loaded', exact: true }),
  ).toBeVisible();
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  await input.fill('cancelled');
  await submit.click();
  await expect.poll(() => requested.includes('cancelled')).toBe(true);
  await expect(progress(page)).toHaveAttribute('data-pending', 'true');
  await page.getByRole('button', { name: 'Reset filters', exact: true }).click();
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  cancelled.resolve();
  await page.goBack();
  await expect(page).toHaveURL(/q=cancelled/);
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  await page.goForward();
  await expect(page).toHaveURL(/\/beatmaps$/);
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
});

for (const support of ['native', 'fallback', 'reduced'] as const) {
  test(`route navigation settles with ${support} view transitions and preserves the active indicator`, async ({
    page,
  }, info) => {
    if (support === 'reduced') await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.addInitScript(
      ({ disable }) => {
        const original = document.startViewTransition?.bind(document);
        Object.assign(window, {
          navigationViewAvailable: Boolean(original),
          navigationViewCalls: 0,
          navigationViewDone: 0,
          navigationViewFinished: 0,
        });
        Object.defineProperty(document, 'startViewTransition', {
          configurable: true,
          value:
            disable || !original
              ? undefined
              : (callback: ViewTransitionUpdateCallback) => {
                  const state = window as typeof window & {
                    navigationViewCalls: number;
                    navigationViewDone: number;
                    navigationViewFinished: number;
                  };
                  state.navigationViewCalls++;
                  const transition = original(callback);
                  void transition.updateCallbackDone.then(() => {
                    state.navigationViewDone++;
                  });
                  void transition.finished.then(() => {
                    state.navigationViewFinished++;
                  });
                  return transition;
                },
        });
      },
      { disable: support === 'fallback' },
    );
    await fixture(page);
    const hold = deferred();
    await page.route('**/connect*', async (route) => {
      if (new URL(route.request().url()).pathname !== '/connect') return route.continue();
      await hold.promise;
      await route.continue();
    });
    await page.goto('/beatmaps');
    await expect(progress(page)).toHaveAttribute('data-pending', 'false');
    await page.locator('.desktop-nav').getByRole('link', { name: 'Connect', exact: true }).click();
    await expect(progress(page)).toHaveAttribute('data-pending', 'true');
    if (support === 'reduced') {
      await expect(progress(page)).toHaveCSS('opacity', '1');
      await page.screenshot({ path: info.outputPath('navigation-pending-reduced.png') });
    }
    hold.resolve();
    await expect(page).toHaveURL(/\/connect$/);
    await expect(progress(page)).toHaveAttribute('data-pending', 'false');
    await expect(page.locator('.desktop-nav a[aria-current="page"]')).toHaveAttribute(
      'href',
      '/connect',
    );
    await expect(page.locator('.desktop-nav .navigation-active-indicator')).toBeVisible();
    const counts = await page.evaluate(() => {
      const state = window as typeof window & {
        navigationViewAvailable: boolean;
        navigationViewCalls: number;
        navigationViewDone: number;
        navigationViewFinished: number;
      };
      return {
        available: state.navigationViewAvailable,
        calls: state.navigationViewCalls,
        done: state.navigationViewDone,
        finished: state.navigationViewFinished,
      };
    });
    if (support === 'native' && counts.available) {
      expect(counts.calls).toBeGreaterThan(0);
      await expect
        .poll(() =>
          page.evaluate(() => {
            const state = window as typeof window & {
              navigationViewDone: number;
              navigationViewFinished: number;
            };
            return [state.navigationViewDone, state.navigationViewFinished];
          }),
        )
        .toEqual([counts.calls, counts.calls]);
    } else expect(counts.calls).toBe(0);
    await page.goBack();
    await expect(page).toHaveURL(/\/beatmaps$/);
    await expect(progress(page)).toHaveAttribute('data-pending', 'false');
    await page.goForward();
    await expect(page).toHaveURL(/\/connect$/);
    await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  });
}

test('same-page, hash, modified and external links keep ordinary anchor behavior', async ({
  page,
}) => {
  await fixture(page);
  await page
    .context()
    .route('https://github.com/**', (route) =>
      route.fulfill({ status: 200, body: 'External fixture' }),
    );
  await page.goto('/beatmaps');
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  await page.locator('.desktop-nav').getByRole('link', { name: 'Beatmaps', exact: true }).click();
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  await page.locator('.skip-link').focus();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/#main$/);
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  const popupEvent = page.context().waitForEvent('page');
  await page
    .locator('.desktop-nav')
    .getByRole('link', { name: 'Connect', exact: true })
    .click({ modifiers: ['ControlOrMeta'] });
  const popup = await popupEvent;
  await expect(page).toHaveURL(/\/beatmaps#main$/);
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  await popup.close();
  const externalEvent = page.context().waitForEvent('page');
  await page.locator('.site-footer').getByRole('link', { name: 'osu!', exact: true }).click();
  const external = await externalEvent;
  await expect(progress(page)).toHaveAttribute('data-pending', 'false');
  await external.close();
});

for (const gutter of ['classic', 'zero-width'] as const) {
  test(`Select and modal Appearance keep ${gutter} scrollbar geometry and keyboard focus`, async ({
    page,
  }, info) => {
    await fixture(page);
    await page.goto('/beatmaps');
    await expect(progress(page)).toHaveAttribute('data-pending', 'false');
    await page.addStyleTag({
      content: `html { overflow-y: scroll; ${gutter === 'zero-width' ? 'scrollbar-width: none;' : ''} } html::-webkit-scrollbar { width: ${gutter === 'classic' ? '17px' : '0'}; } body::after { content: ''; display: block; height: 110vh; }`,
    });
    const initial = await shellGeometry(page);
    const source = page.getByRole('combobox', { name: 'Source', exact: true });
    await source.focus();
    await page.keyboard.press('ArrowDown');
    await expect(page.getByRole('listbox')).toBeVisible();
    await expect(page.locator('body')).toHaveAttribute('data-scroll-locked');
    await injectLateScrollLockCompensation(page);
    await expect(page.locator('body')).toHaveCSS('margin-right', '0px');
    await expect(page.locator('body')).toHaveCSS('--removed-body-scroll-bar-size', '0px');
    await expectStableShell(page, initial);
    await page.screenshot({ path: info.outputPath(`navigation-select-${gutter}.png`) });
    await page.keyboard.press('Escape');
    await expect(source).toBeFocused();
    await expectStableShell(page, initial);
    const appearance = page.getByRole('button', { name: 'Appearance', exact: true });
    await expect(appearance).toHaveCount(1);
    await appearance.click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(page.locator('body')).toHaveAttribute('data-scroll-locked');
    await injectLateScrollLockCompensation(page);
    await expect(page.locator('body')).toHaveCSS('margin-right', '0px');
    await expect(page.locator('body')).toHaveCSS('--removed-body-scroll-bar-size', '0px');
    await expectStableShell(page, initial);
    for (const name of ['Dark', 'Light', 'System'])
      await expect(dialog.getByRole('button', { name, exact: true })).toBeVisible();
    await dialog.getByRole('button', { name: 'Light', exact: true }).click();
    await expect(page.locator('html')).toHaveClass(/light/);
    await dialog.getByRole('button', { name: 'Dark', exact: true }).click();
    await page.keyboard.press('Escape');
    await expect(appearance).toBeFocused();
    await expectStableShell(page, initial);
  });
}

test('nonmodal score DropdownMenu preserves scroll and focus', async ({ page }, info) => {
  await fixture(page);
  await page.goto(`/players/${playerId}`);
  const trigger = page.getByRole('button', {
    name: 'Score actions: Navigation score fixture',
    exact: true,
  });
  await expect(trigger).toBeVisible();
  await page.addStyleTag({
    content:
      "html { overflow-y: scroll; } html::-webkit-scrollbar { width: 17px; } body::after { content: ''; display: block; height: 110vh; }",
  });
  const initial = await shellGeometry(page);
  await trigger.focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menu')).toBeVisible();
  await expect(page.getByRole('menuitem', { name: 'View score', exact: true })).toBeFocused();
  await expect(page.locator('body')).not.toHaveAttribute('data-scroll-locked');
  await expectStableShell(page, initial);
  await page.screenshot({ path: info.outputPath('navigation-dropdown-open.png') });
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
  await expectStableShell(page, initial);
});

test('mobile menu preserves modal focus, geometry and single Appearance access', async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await fixture(page);
  await page.goto('/beatmaps');
  const initial = await shellGeometry(page);
  const trigger = page.locator('.mobile-menu');
  await trigger.click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await expectStableShell(page, initial);
  await page.screenshot({ path: info.outputPath('navigation-mobile-menu.png') });
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
  await expectStableShell(page, initial);
  await expect(page.getByRole('button', { name: 'Appearance', exact: true })).toHaveCount(1);
});
