import { defineConfig } from '@playwright/test'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const snapshotDir = mkdtempSync(join(tmpdir(), 'vela-browser-test-'))

export default defineConfig({
  testDir: './e2e',
  workers: 1,
  use: { baseURL: 'http://localhost:3000', channel: 'chrome' },
  webServer: [
    {
      command: 'cargo run --quiet -p api --bin api',
      cwd: '..',
      url: 'http://localhost:3001/health',
      timeout: 180_000,
      env: { ADMIN_TOKEN: 'local-browser-test', VELA_RUNTIME_MODE: 'testnet-demo', SNAPSHOT_DIR: snapshotDir, PORT: '3001', DATABASE_URL: '' },
    },
    {
      command: 'npm run dev -- --port 3000',
      url: 'http://localhost:3000/agents',
      timeout: 120_000,
      env: { NEXT_PUBLIC_API_URL: 'http://localhost:3001' },
    },
  ],
})
