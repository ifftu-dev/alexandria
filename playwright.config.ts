import { defineConfig } from '@playwright/test'

const port = process.env.DESKTOP_UI_PORT ?? '5184'
/** Specs that hold the main thread to a time budget. */
const PERFORMANCE = '**/atlas-performance.spec.ts'

export default defineConfig({
  testDir: './tests/desktop-ui',
  testMatch: '**/*.spec.ts',
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 2 : undefined,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
    viewport: { width: 1280, height: 800 },
  },
  projects: [
    { name: 'chromium', use: { browserName: 'chromium' }, testIgnore: PERFORMANCE },
    { name: 'webkit', use: { browserName: 'webkit' }, testIgnore: PERFORMANCE },
    // Main-thread budgets measure wall time, which browsers running beside
    // them stretch: they run after the rest, alone, one test at a time.
    { name: 'performance', use: { browserName: 'chromium' }, testMatch: PERFORMANCE, dependencies: ['chromium', 'webkit'], fullyParallel: false, workers: 1 },
  ],
  webServer: {
    command: `npm run dev -- --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}/tests/desktop-ui/`,
    reuseExistingServer: !process.env.CI,
  },
})
