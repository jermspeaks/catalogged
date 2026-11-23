import { defineConfig } from '@playwright/test';
import path from 'node:path';
import { AUD, TEAM, goodKey, mint } from './keys';

goodKey(); // make sure the key files exist before the JWKS server starts

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
  // The Worker runs with real Access verification on; every request carries a valid fake token.
  use: { baseURL: `http://localhost:${PORT}`, trace: 'retain-on-failure', extraHTTPHeaders: { 'Cf-Access-Jwt-Assertion': mint() } },
  webServer: [{
    command: 'node jwks-server.mjs',
    cwd: __dirname,
    url: 'http://127.0.0.1:8790/',
    reuseExistingServer: false,
  }, {
    // Fresh state every run, then migrations, then the Worker. DISABLE_CACHE keeps pages fresh
    // between the test's writes and reads (the Cache API would otherwise serve stale pages).
    command:
      `rm -rf "${state}" && ` +
      `npx wrangler d1 migrations apply catalogged --local --persist-to "${state}" && ` +
      `npx wrangler dev --port ${PORT} --persist-to "${state}" ` +
      `--var ADMIN_ENABLED:1 --var DISABLE_CACHE:1 --var COVERS_BASE_URL: ` +
      `--var ACCESS_JWT_OPTIONAL: --var ACCESS_TEAM_DOMAIN:${TEAM} --var ACCESS_AUD:${AUD} ` +
      `--var ACCESS_JWKS_URL:http://127.0.0.1:8790/certs`,
    cwd: path.resolve(__dirname, '../crates/worker'),
    url: `http://localhost:${PORT}/healthz`,
    timeout: 420_000,
    reuseExistingServer: false,
    stdout: 'ignore',
    stderr: 'pipe',
  }],
});
