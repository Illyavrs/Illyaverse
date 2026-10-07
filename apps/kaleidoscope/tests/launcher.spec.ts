import { test, expect, type Page } from '@playwright/test';
import type { LauncherSnapshot } from '../src/lib/runtime';

const ready: LauncherSnapshot = {
  stable: { path: 'C:\\Games\\osu!\\osu!.exe', running: false },
  lazer: { path: 'C:\\Users\\Player\\AppData\\Local\\osulazer\\current\\osu!.exe', running: false, compatibility: 'supported' },
  phase: 'idle', detail: '遊戲位置已確認，可以啟動。', pluginPresent: false,
};

async function mockNative(page: Page, state: LauncherSnapshot) {
  await page.addInitScript((initial) => {
    let snapshot = structuredClone(initial);
    const commands: { command: string; args?: { client: 'stable' | 'lazer' } }[] = [];
    Object.assign(window, {
      isTauri: true,
      __launcherTestCommands: commands,
      __TAURI_INTERNALS__: {
        async invoke(command: string, args?: { client: 'stable' | 'lazer' }) {
          commands.push({ command, args });
          if (command === 'launcher_start' && args) {
            await new Promise((resolve) => setTimeout(resolve, 120));
            snapshot[args.client].running = true;
            snapshot.phase = 'running';
            snapshot.pluginPresent = snapshot.lazer.running;
            snapshot.detail = `${args.client} 已啟動。`;
          } else if (command === 'launcher_stop' || command === 'launcher_recover') {
            await new Promise((resolve) => setTimeout(resolve, 120));
            snapshot = { ...snapshot, stable: { ...snapshot.stable, running: false }, lazer: { ...snapshot.lazer, running: false }, phase: 'idle', detail: '遊戲已關閉，連線 ruleset 已清理。', pluginPresent: false };
          }
          return structuredClone(snapshot);
        },
      },
    });
  }, state);
}

async function capture(page: Page, name: string) {
  await page.evaluate(() => window.scrollTo(0, 0));
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  expect(await page.locator('body').evaluate((node) => getComputedStyle(node).backgroundColor)).toBe('rgb(0, 0, 0)');
  if (process.env.ILLYA_CAPTURE_LAUNCHER_REVIEW === '1') {
    await page.screenshot({ path: `../../data-launcher-review/${name}.png`, fullPage: true, animations: 'disabled' });
  }
}

test('development browser preview labels its fixture and simulates choosing, running and cleanup', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 720, height: 640 });
  await page.goto('/');
  await expect(page.getByText('介面預覽 · 按鈕僅模擬狀態，不會啟動遊戲')).toBeVisible();
  await expect(page.getByRole('button', { name: '啟動 lazer', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: '選擇 lazer 安裝位置' }).click();
  await expect(page.getByRole('textbox', { name: 'lazer 安裝位置' })).toHaveValue(/osulazer/);
  await page.getByRole('button', { name: '啟動 lazer', exact: true }).click();
  await expect(page.getByRole('button', { name: '關閉並重新啟動 lazer' })).toBeEnabled();
  await expect(page.getByTestId('cleanup-status')).toHaveText('連線 ruleset 使用中');
  await page.getByRole('button', { name: '關閉遊戲', exact: true }).click();
  await expect(page.getByTestId('cleanup-status')).toHaveText('沒有暫存的連線 ruleset');
  expect(errors).toEqual([]);
});

test('native bridge shows ready and running states with one fixed server and keyboard controls', async ({ page }) => {
  await mockNative(page, ready);
  await page.setViewportSize({ width: 720, height: 640 });
  await page.goto('/');
  await expect(page.getByRole('button', { name: '啟動 stable', exact: true })).toBeEnabled();
  await expect(page.getByText('介面預覽', { exact: false })).toHaveCount(0);
  await expect(page.getByRole('combobox')).toHaveCount(0);
  await capture(page, 'launcher-ready-desktop');
  await page.getByRole('button', { name: '啟動 lazer', exact: true }).focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('button', { name: '關閉並重新啟動 lazer' })).toBeEnabled();
  await capture(page, 'launcher-running-desktop');
  await page.setViewportSize({ width: 390, height: 844 });
  await capture(page, 'launcher-running-mobile');
  await page.getByRole('button', { name: '關閉並重新啟動 lazer' }).click();
  await expect(page.getByRole('button', { name: '關閉並重新啟動 lazer' })).toBeEnabled();
  const commands = await page.evaluate(() => (window as unknown as { __launcherTestCommands: { command: string; args?: { client: string } }[] }).__launcherTestCommands);
  expect(commands.filter((entry) => entry.command === 'launcher_start')).toEqual([
    { command: 'launcher_start', args: { client: 'lazer' } }, { command: 'launcher_start', args: { client: 'lazer' } },
  ]);
});

test('unsupported lazer and failed cleanup block launching while cleanup remains recoverable', async ({ page }) => {
  await mockNative(page, { ...ready, phase: 'cleanup_failed', pluginPresent: true, detail: '連線 ruleset 無法刪除。請關閉 lazer 後重試清理。' });
  await page.setViewportSize({ width: 720, height: 640 });
  await page.goto('/');
  await expect(page.getByRole('button', { name: '啟動 lazer', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: '啟動 stable', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: '重試清理' })).toBeEnabled();
  await capture(page, 'launcher-cleanup-failed-desktop');
  await page.setViewportSize({ width: 390, height: 844 });
  await capture(page, 'launcher-cleanup-failed-mobile');
  await page.getByRole('button', { name: '重試清理' }).click();
  await expect(page.getByTestId('cleanup-status')).toHaveText('沒有暫存的連線 ruleset');
  await expect(page.getByRole('button', { name: '啟動 lazer', exact: true })).toBeEnabled();
  await capture(page, 'launcher-ready-mobile');

  const unsupported = await page.context().newPage();
  await mockNative(unsupported, { ...ready, lazer: { ...ready.lazer, compatibility: 'unsupported' } });
  await unsupported.goto('/');
  await expect(unsupported.getByRole('button', { name: '啟動 lazer', exact: true })).toBeDisabled();
  await expect(unsupported.getByText('此版本暫不支援，請等待連線 ruleset 更新。')).toBeVisible();
  await unsupported.close();
});

test('a malformed native snapshot fails closed without enabling preview actions', async ({ page }) => {
  await page.addInitScript(() => Object.assign(window, {
    isTauri: true, __TAURI_INTERNALS__: { async invoke() { return { phase: 'idle' }; } },
  }));
  await page.goto('/');
  await expect(page.getByText('無法連接啟動器。請重新開啟桌面程式。')).toBeVisible();
  await expect(page.getByRole('button', { name: '選擇 lazer 安裝位置' })).toBeDisabled();
  await expect(page.getByText('介面預覽', { exact: false })).toHaveCount(0);
});
