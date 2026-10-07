import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests', workers: 1, fullyParallel: false, retries: 0, timeout: 30000,
  use: { baseURL: process.env.ILLYA_LAUNCHER_URL ?? 'http://localhost:3010', trace: 'off', screenshot: 'only-on-failure' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
