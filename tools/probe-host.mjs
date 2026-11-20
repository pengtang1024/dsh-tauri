#!/usr/bin/env node
/**
 * Probe: prove the upstream DSH desktop Host can be started and authenticated
 * outside Electron, using the exact spawn contract reverse-engineered from
 * `resources/app.asar:/lib/desktop-runtime-*.js` and `/lib/host-process-*.js`.
 *
 * Steps: spawn Host -> wait for `ready` over Node IPC -> exchange the launch URL
 * for a cookie (303 + set-cookie) -> fetch the Web application root with it.
 *
 * Usage:
 *   node tools/probe-host.mjs [--home <dir>] [--keep]
 */
import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';

const APP_ROOT = process.env.DSH_PROBE_APP_ROOT ?? '/opt/DSH NEXT/resources/app.asar';
const RESOURCES = process.env.DSH_PROBE_RESOURCES ?? '/opt/DSH NEXT/resources';
const EXECUTABLE = process.env.DSH_PROBE_EXECUTABLE ?? '/opt/DSH NEXT/dsh-desktop-next';
const PRIMARY_RUNTIME = join(RESOURCES, 'runtime', 'x64', 'primary-runtime');
const REAL_HOME = join(homedir(), '.dsh');
const LAUNCH_TIMEOUT_MS = Number(process.env.DSH_PROBE_TIMEOUT ?? 150_000);

const args = process.argv.slice(2);
const keep = args.includes('--keep');
const homeIndex = args.indexOf('--home');
const probeHome = homeIndex >= 0 ? args[homeIndex + 1] : join(tmpdir(), `dsh-probe-home-${process.pid}`);

const prefs = {
  closeToTray: true,
  macosMaterial: 'transparent',
  windowsMaterial: 'off',
  linuxMaterial: 'off',
  browserAccess: false,
  networkExposure: 'loopback',
  port: 0,
  lanPort: 0,
  logLevel: 'info',
  developerLogging: false,
  sidebarFooterCompat: true,
  sidebarFooterBalanced: true,
  notifications: false,
  turnCompleted: false,
  turnFailed: false,
  jobCompleted: false,
  jobFailed: false,
  scheduleCompleted: false,
  scheduleFailed: false,
  hostStartupTimeout: 120,
  appIcon: 'default'
};

const token = randomBytes(32).toString('base64url');

function prepareHome() {
  if (!existsSync(probeHome)) mkdirSync(probeHome, { recursive: true });
  const profile = join(probeHome, 'profiles', 'desktop');
  if (!existsSync(profile)) mkdirSync(profile, { recursive: true });
  // Seed the probe profile with the real one's configuration (never its data).
  const realProfile = join(REAL_HOME, 'profiles', 'desktop');
  for (const name of [
    'cordis.yml',
    'cordis.patch.yml',
    'desktop-next.cordis.patch.json'
  ]) {
    const from = join(realProfile, name);
    const to = join(profile, name);
    if (existsSync(from) && !existsSync(to)) cpSync(from, to);
  }
  // The Host reads the Web client dist from its own node_modules, so the
  // profile only needs its configuration.
  return profile;
}

function log(...parts) {
  console.log('[probe]', ...parts);
}

const profile = prepareHome();
log('app root     :', APP_ROOT);
log('profile dir  :', profile);
log('primary runt :', PRIMARY_RUNTIME);
log('executable   :', EXECUTABLE);

const child = spawn(
  EXECUTABLE,
  [
    '--expose-internals',
    '--max-http-header-size=65536',
    join(APP_ROOT, 'lib', 'host.js'),
    APP_ROOT,
    profile,
    PRIMARY_RUNTIME
  ],
  {
    cwd: profile,
    env: {
      ...process.env,
      ELECTRON_RUN_AS_NODE: '1',
      DSH_HOME: probeHome,
      DSH_NEXT_NATIVE_TOKEN: token,
      DSH_NEXT_PREFERENCES: JSON.stringify(prefs),
      DSH_NEXT_TRUSTED_HOSTS: '[]',
      DSH_TELEMETRY_DISABLED: '1'
    },
    stdio: ['ignore', 'pipe', 'pipe', 'ipc']
  }
);

let stderr = '';
child.stderr.setEncoding('utf8');
child.stderr.on('data', (chunk) => {
  stderr = (stderr + chunk).slice(-8000);
  process.stderr.write('[host] ' + chunk);
});
child.stdout.setEncoding('utf8');
child.stdout.on('data', (chunk) => process.stdout.write('[host-out] ' + chunk));

let timer;
const ready = new Promise((resolve, reject) => {
  timer = setTimeout(() => reject(new Error('timed out waiting for the Host ready event')), LAUNCH_TIMEOUT_MS);
  child.on('message', (message) => {
    if (message && message.type === 'ready') resolve(message);
  });
  child.on('exit', (code, signal) =>
    reject(new Error(`Host exited before ready (code=${code} signal=${signal})\n${stderr}`))
  );
});

async function main() {
  const event = await ready;
  clearTimeout(timer);
  log('ready.url    :', event.url);
  log('injections   :', Array.isArray(event.injections) ? `${event.injections.length} item(s)` : String(event.injections));

  // 1. Exchange the launch URL for the Host's authority-bound cookie.
  const auth = await fetch(event.url, {
    redirect: 'manual',
    headers: { 'x-dsh-desktop-renderer': token }
  });
  const setCookie = auth.headers.get('set-cookie');
  await auth.body?.cancel();
  log('auth status  :', auth.status, '| set-cookie:', setCookie ? 'present' : 'MISSING');
  if (auth.status !== 303 || !setCookie) throw new Error('authentication contract mismatch');
  const cookie = setCookie.slice(0, setCookie.indexOf(';') < 0 ? undefined : setCookie.indexOf(';'));

  // 2. Fetch the Web application root through the authenticated cookie.
  const nativeHeaders = { cookie, 'x-dsh-desktop-renderer': token };
  const index = await fetch(new URL('/', event.url), { headers: nativeHeaders });
  const html = await index.text();
  log('index status :', index.status, '| bytes:', html.length, '| <div id="root">:', html.includes('id="root"'));

  // 3. Fetch one API route to prove the reverse-proxy path works.
  const api = await fetch(new URL('/api/health', event.url), { headers: nativeHeaders }).catch((error) => ({ status: 'ERR', text: async () => String(error) }));
  log('api /health  :', api.status);

  if (index.status !== 200 || !html.includes('id="root"')) throw new Error('index response mismatch');

  // 4. Optional: dump the client bundle and report its transport usage.
  const dumpIndex = args.indexOf('--dump');
  if (dumpIndex >= 0) {
    const dir = args[dumpIndex + 1] ?? join(tmpdir(), 'dsh-client-dump');
    mkdirSync(dir, { recursive: true });
    const { writeFileSync } = await import('node:fs');
    writeFileSync(join(dir, 'index.html'), html);
    const sources = [...html.matchAll(/(?:src|href)="([^"]+)"/g)]
      .map((m) => m[1].replace(/&amp;/g, '&'))
      .filter((u) => !/^(?:[a-z]+:|#)/i.test(u))
      .map((u) => (u.startsWith('/') ? u : '/' + u));
    log('sources      :', sources.slice(0, 4));
    log('dump dir     :', dir, '| assets:', sources.length);
    const evidence = { WebSocket: [], EventSource: [] };
    for (const source of sources) {
      const asset = await fetch(new URL(source, event.url), { headers: nativeHeaders });
      if (!asset.ok) continue;
      const body = await asset.text();
      if (evidence.WebSocket.length + evidence.EventSource.length < 8) {
        writeFileSync(join(dir, source.replace(/[^a-z0-9]+/gi, '_').slice(-80)), body);
      }
      for (const kind of ['WebSocket', 'EventSource']) {
        let at = body.indexOf(kind + '(');
        while (at >= 0 && evidence[kind].length < 6) {
          evidence[kind].push(body.slice(Math.max(0, at - 120), at + 140).replace(/\s+/g, ' '));
          at = body.indexOf(kind + '(', at + 1);
        }
      }
    }
    for (const [kind, hits] of Object.entries(evidence)) {
      log(kind, 'sites:', hits.length);
      for (const hit of hits.slice(0, 4)) log('   ', hit);
    }
  }

  log('RESULT: OK — Host boots and authenticates without Electron');

  // Ask the Host to shut down cleanly, then wait briefly.
  try {
    child.send({ type: 'shutdown' });
  } catch {}
  await new Promise((resolve) => {
    const stop = setTimeout(resolve, 8000);
    child.once('exit', () => {
      clearTimeout(stop);
      resolve();
    });
  });
}

main()
  .catch((error) => {
    console.error('[probe] FAILED:', error.message);
    process.exitCode = 1;
  })
  .finally(async () => {
    clearTimeout(timer);
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
    if (!keep) {
      try {
        rmSync(probeHome, { recursive: true, force: true });
      } catch {}
    } else {
      log('kept probe home:', probeHome);
    }
  });
