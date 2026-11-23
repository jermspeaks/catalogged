import { defineConfig } from '@playwright/test';
import path from 'node:path';

// Everything runs against a local `wrangler dev` with a throwaway D1/R2 in e2e/.state:
// no network services of ours, no production data, no Cloudflare usage.
const state = path.resolve(__dirname, '.state');
const PORT = 8788;

export default defineConfig({
  testDir: './tests',
  workers: 1, // one shared local database
  fullyParallel: false,
  retries: 0,
  reporter: [['list']],
  use: { baseURL: `http://localhost:${PORT}`, trace: 'retain-on-failure' },
  webServer: {
    // Fresh state every run, then migrations, then the Worker. DISABLE_CACHE keeps pages fresh
    // between the test's writes and reads (the Cache API would otherwise serve stale pages).
    command:
      `rm -rf "${state}" && ` +
      `npx wrangler d1 migrations apply catalogged --local --persist-to "${state}" && ` +
      `npx wrangler dev --port ${PORT} --persist-to "${state}" ` +
      `--var ADMIN_ENABLED:1 --var DISABLE_CACHE:1 --var COVERS_BASE_URL:`,
    cwd: path.resolve(__dirname, '../crates/worker'),
    url: `http://localhost:${PORT}/healthz`,
    timeout: 420_000,
    reuseExistingServer: false,
    stdout: 'ignore',
    stderr: 'pipe',
  },
});
