#!/usr/bin/env node
/**
 * Contract probe for the runtime this shell drives.
 *
 * Boots `@deepseek-ai/dsh` (`dsh web`) exactly the way the Tauri shell does,
 * then checks the two things the window depends on:
 *
 *   1. the printed URL is a loopback HTTP origin carrying a token,
 *   2. `GET <url>` answers 303 with a session cookie and the following request
 *      returns the client HTML.
 *
 * Usage: node tools/probe-web.mjs [--profile web]
 */
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const flag = (name, fallback) => {
  const index = process.argv.indexOf(`--${name}`);
  return index === -1 ? fallback : process.argv[index + 1];
};
const runtime = flag('runtime', process.env.DSH_TAURI_RUNTIME ?? join(root, 'runtime'));
const entry = join(runtime, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js');
const profile = flag('profile', 'web');

// A runtime that never prints its URL must not wedge CI forever. Tests shorten
// this budget through DSH_PROBE_TIMEOUT_MS.
const configuredTimeout = Number(process.env.DSH_PROBE_TIMEOUT_MS);
const STARTUP_TIMEOUT = Number.isFinite(configuredTimeout) && configuredTimeout > 0
  ? configuredTimeout
  : 60000;
// No single HTTP exchange may outlive the whole startup budget.
const FETCH_TIMEOUT = Math.min(15000, STARTUP_TIMEOUT);

if (!existsSync(entry)) {
  console.error(`missing runtime: ${entry}\n run: npm install --prefix runtime @deepseek-ai/dsh`);
  process.exit(2);
}

const child = spawn(
  process.env.DSH_TAURI_NODE ?? 'node',
  [entry, '--profile', profile, '--host', '127.0.0.1', '--port', '0', '--no-open'],
  { env: { ...process.env, DSH_HOME: process.env.DSH_HOME ?? join(process.env.HOME ?? '/', '.dsh') }, stdio: ['ignore', 'pipe', 'inherit'] }
);

let finished = false;
let startupTimer = null;

// Idempotent: the exit event and a failed fetch may both report the same fault.
const fail = (message) => {
  if (finished) return;
  finished = true;
  if (startupTimer) clearTimeout(startupTimer);
  console.error(`FAIL ${message}`);
  child.kill('SIGTERM');
  process.exit(1);
};

const succeed = () => {
  if (finished) return;
  finished = true;
  if (startupTimer) clearTimeout(startupTimer);
  child.kill('SIGTERM');
  process.exit(0);
};

// Never leave the spawned runtime behind when the probe itself is interrupted.
const terminate = (signal) => fail(`received ${signal}`);
process.on('SIGINT', () => terminate('SIGINT'));
process.on('SIGTERM', () => terminate('SIGTERM'));

startupTimer = setTimeout(() => {
  fail(`runtime did not print its URL within ${STARTUP_TIMEOUT}ms (startup timeout)`);
}, STARTUP_TIMEOUT);

let buffer = '';
child.stdout.setEncoding('utf8');
child.stdout.on('data', async (chunk) => {
  buffer += chunk;
  const match = buffer.match(/dsh web:\s+(http:\/\/127\.0\.0\.1:\d+\/\?token=[\w-]+)/);
  if (!match || child.__checked || finished) return;
  child.__checked = true;
  const url = match[1];
  console.log(`url ${url}`);

  try {
    const auth = await fetch(url, { redirect: 'manual', signal: AbortSignal.timeout(FETCH_TIMEOUT) });
    if (auth.status !== 303) return fail(`expected 303 from the token URL, got ${auth.status}`);
    const cookie = auth.headers.get('set-cookie');
    if (!cookie) return fail('the token URL answered without a session cookie');
    console.log(`auth 303 → ${cookie.split(';')[0].split('=')[0]}`);

    const html = await fetch(new URL(url).origin, {
      headers: { cookie: cookie.split(';')[0] },
      signal: AbortSignal.timeout(FETCH_TIMEOUT)
    });
    const body = await html.text();
    if (html.status !== 200) return fail(`expected 200 from /, got ${html.status}`);
    if (!body.includes('id="root"')) return fail('the client HTML does not contain the app root');
    console.log(`root 200 ${body.length}B (client HTML)`);
    console.log('RESULT: OK — the runtime serves the client the shell embeds');
    succeed();
  } catch (error) {
    fail(`client contract probe failed: ${error?.message ?? error}`);
  }
});

child.on('exit', (code) => {
  if (!child.__checked) fail(`runtime exited before printing its URL (code ${code})`);
});
