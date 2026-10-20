import { defineConfig, devices } from '@playwright/test';

// UI visual + layout suite. Runs the Svelte app against the in-memory mock IPC (vite dev server),
// in chromium AND webkit (the real app is WKWebView on macOS). Port 1431: never collides with
// `just dev` (1420). Artifacts for the gallery report land in e2e/out (gitignored).
export default defineConfig({
  testDir: './e2e',
  testMatch: /.*\.spec\.ts/,
  outputDir: 'e2e/out/pw',
  snapshotPathTemplate: '{testDir}/baselines/{projectName}/{arg}{ext}',
  fullyParallel: true,
  workers: process.env.CI ? 2 : 4,
  retries: 0,
  reporter: [['list'], ['json', { outputFile: 'e2e/out/results.json' }]],
  expect: { toHaveScreenshot: { maxDiffPixelRatio: 0.01, animations: 'disabled', caret: 'hide' } },
  use: { baseURL: 'http://127.0.0.1:1431', colorScheme: 'dark', reducedMotion: 'reduce', deviceScaleFactor: 1 },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], deviceScaleFactor: 1 } },
    { name: 'webkit', use: { ...devices['Desktop Safari'], deviceScaleFactor: 1 } },
  ],
  webServer: {
    command: 'pnpm exec vite --port 1431 --strictPort --host 127.0.0.1',
    url: 'http://127.0.0.1:1431/main.html',
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
