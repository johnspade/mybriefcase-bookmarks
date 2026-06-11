import { test as base, type Page } from '@playwright/test';
import { execSync, spawn, type ChildProcess } from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as net from 'net';

const PROJECT_ROOT = path.resolve(__dirname, '..');

function getBinaryPath(): string {
  try {
    const meta = JSON.parse(
      execSync('cargo metadata --format-version 1 --no-deps', {
        cwd: PROJECT_ROOT,
        encoding: 'utf-8',
        stdio: ['ignore', 'pipe', 'ignore'],
      })
    );
    return path.join(meta.target_directory, 'release', 'automerge-playground');
  } catch {
    return path.join(PROJECT_ROOT, 'target', 'release', 'automerge-playground');
  }
}

async function getAvailablePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address() as net.AddressInfo;
      server.close(() => resolve(port));
    });
    server.on('error', reject);
  });
}

async function waitForServer(port: number, timeoutMs = 30_000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const response = await fetch(`http://localhost:${port}/`);
      if (response.ok) return;
    } catch {
      // not ready yet
    }
    await new Promise(r => setTimeout(r, 100));
  }
  throw new Error(`Server did not start within ${timeoutMs}ms`);
}

type ServerFixture = {
  serverPage: Page;
};

export const test = base.extend<ServerFixture>({
  serverPage: [async ({ browser }, use) => {
    const port = await getAvailablePort();
    const syncRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'bm-e2e-sync-'));
    const localDir = fs.mkdtempSync(path.join(os.tmpdir(), 'bm-e2e-local-'));
    const binaryPath = getBinaryPath();

    if (!fs.existsSync(binaryPath)) {
      throw new Error(`Binary not found at ${binaryPath}. Run 'cargo build --release' first.`);
    }

    const serverProcess: ChildProcess = spawn(binaryPath, [], {
      env: {
        ...process.env,
        BOOKMARK_PORT: String(port),
        BOOKMARK_SYNC_ROOT: syncRoot,
        BOOKMARK_LOCAL_DIR: localDir,
        BOOKMARK_DEV_MODE: '1',
        BOOKMARK_CLIENT_ID: 'e2e-test',
      },
      cwd: PROJECT_ROOT,
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    serverProcess.stderr?.on('data', (data) => {
      if (process.env.DEBUG) {
        process.stderr.write(`[server] ${data}`);
      }
    });

    await waitForServer(port);

    const context = await browser.newContext({ baseURL: `http://localhost:${port}` });
    const page = await context.newPage();

    await use(page);

    await context.close();
    serverProcess.kill('SIGTERM');
    await new Promise<void>(resolve => {
      serverProcess.on('exit', () => resolve());
      setTimeout(() => { serverProcess.kill('SIGKILL'); resolve(); }, 5000);
    });

    fs.rmSync(syncRoot, { recursive: true, force: true });
    fs.rmSync(localDir, { recursive: true, force: true });
  }, { scope: 'test' }],
});

export { expect } from '@playwright/test';
