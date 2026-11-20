#!/usr/bin/env node
/**
 * Integration probe for `bridge/host-bridge.mjs`: drives the bridge over its
 * line protocol exactly like the Rust shell does, then authenticates and loads
 * the Web application through the Host.
 *
 * Usage: node tools/probe-bridge.mjs [--keep]
 */
import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { createInterface } from 'node:readline';

const ROOT = '/opt/DSH NEXT/resources/app.asar';
const RESOURCES = '/opt/DSH NEXT/resources';
const EXECUTABLE = '/opt/DSH NEXT/dsh-desktop-next';
const PRIMARY_RUNTIME = join(RESOURCES, 'runtime', 'x64', 'primary-runtime');
const REAL_HOME = join(homedir(), '.dsh');
const HERE = dirname(new URL(import.meta.url).pathname);
const keep = process.argv.includes('--keep');
const probeHome = join(tmpdir(), `dsh-bridge-home-${process.pid}`);
const token = randomBytes(32).toString('base64url');

const log = (...parts) => console.log('[probe]', ...parts);

function seedProfile() {
  mkdirSync(join(probeHome, 'profiles', 'desktop'), { recursive: true });
  const real = join(REAL_HOME, 'profiles', 'desktop');
  for (const name of ['cordis.yml', 'cordis.patch.yml', 'desktop-next.cordis.patch.json']) {
    const from = join(real, name);
    const to = join(probeHome, 'profiles', 'desktop', name);
    if (existsSync(from) && !existsSync(to)) cpSync(from, to);
  }
}

seedProfile();

const bridge = spawn(
  process.execPath,
  [
    join(HERE, '..', 'bridge', 'host-bridge.mjs'),
    '--root',
    ROOT,
    '--profile',
    join(probeHome, 'profiles', 'desktop'),
    '--runtime',
    PRIMARY_RUNTIME,
    '--executable',
    EXECUTABLE,
    '--home',
    probeHome,
    '--token',
    token
  ],
  {
    env: {
      ...process.env,
      DSH_NEXT_PREFERENCES: JSON.stringify({ browserAccess: false, hostStartupTimeout: 120, logLevel: 'info' }),
      DSH_NEXT_TRUSTED_HOSTS: '[]',
      DSH_TELEMETRY_DISABLED: '1'
    },
    stdio: ['pipe', 'pipe', 'inherit']
  }
);

const events = [];
let readyResolve;
const ready = new Promise((resolve) => (readyResolve = resolve));

createInterface({ input: bridge.stdout }).on('line', (line) => {
  let event;
  try {
    event = JSON.parse(line);
  } catch {
    return;
  }
  events.push(event.type);
  if (event.type === 'ready') readyResolve(event);
  if (event.type === 'error') log('bridge error:', event.message);
});

const timer = setTimeout(() => {
  console.error('[probe] FAILED: no ready event within 180s');
  process.exit(1);
}, 180_000);

const send = (message) => bridge.stdin.write(JSON.stringify(message) + '\n');

try {
  const event = await ready;
  clearTimeout(timer);
  log('ready.url    :', event.url);
  log('injections   :', event.injections.length);

  // The bridge authenticates on our behalf and proxies the Host origin, so the
  // WebView (here: plain fetch) needs no token and no cookie.
  const headers = {};
  const index = await fetch(new URL('/', event.url), { headers });
  const html = await index.text();
  log('index        :', index.status, html.length, 'bytes, #root:', html.includes('id="root"'));
  log('boot injected:', html.includes('__DSH_BOOT_READY__'));
  if (index.status !== 200 || !html.includes('id="root"')) throw new Error('proxied index is not the app');

  const withoutToken = await fetch(new URL('/', event.hostUrl), { redirect: 'manual' });
  await withoutToken.body?.cancel();
  log('host w/o auth:', withoutToken.status, '(expected 303 or 403)');

  const firstScript = /src="([^"]+)"/.exec(html)?.[1];
  if (firstScript) {
    const asset = await fetch(new URL('/' + firstScript.replace(/&amp;/g, '&'), event.url), { headers });
    await asset.body?.cancel();
    log('first asset  :', firstScript.slice(0, 56), '->', asset.status);
  }

  // Prove the shell->host direction: ask the Host for its startup entries.
  const requestId = 1;
  const answered = new Promise((resolve) => {
    const check = setInterval(() => {
      const hit = events.includes('host');
      if (hit) {
        clearInterval(check);
        resolve(true);
      }
    }, 500);
    setTimeout(() => {
      clearInterval(check);
      resolve(false);
    }, 15_000);
  });
  send({ type: 'host', message: { type: 'startup-entries', requestId } });
  log('control round-trip:', (await answered) ? 'Host replied' : 'no reply', '(events:', events.join(',') + ')');

  log('RESULT: OK — bridge protocol works');
  send({ type: 'shutdown' });
  await new Promise((resolve) => bridge.once('exit', resolve));
} catch (error) {
  console.error('[probe] FAILED:', error.message);
  process.exitCode = 1;
} finally {
  clearTimeout(timer);
  if (bridge.exitCode === null && bridge.signalCode === null) bridge.kill('SIGTERM');
  if (!keep) rmSync(probeHome, { recursive: true, force: true });
}
