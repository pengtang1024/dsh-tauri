#!/usr/bin/env node
/**
 * Contract probe for the runtime this shell drives.
 *
 * Boots `@deepseek-ai/dsh` (`dsh web`) exactly the way the Tauri shell does,
 * then checks the two things the window depends on:
 *
 *   1. the printed URL is a loopback HTTP origin carrying a token,
 *   2. `GET <url>` answers 303 with a same-origin Location and a session cookie,
 *      and the following request returns the client HTML.
 *
 * Usage: node tools/probe-web.mjs [--profile web]
 */
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bannerUrl } from './banner-url.mjs';

/**
 * 认证重定向目标是否仍落在 token URL 的 origin（纯函数，无副作用、不打印）。
 * 相对 Location 按 tokenUrl 解析；解析失败或外部 origin 一律返回 false。
 */
export function redirectIsLocal(location, tokenUrl) {
  if (typeof location !== 'string' || location.trim() === '') return false;
  try {
    return new URL(location, tokenUrl).origin === new URL(tokenUrl).origin;
  } catch {
    return false;
  }
}

const self = fileURLToPath(import.meta.url);
// 让单测可以 import 纯函数而不真的拉起运行时探针。
const invokedDirectly = Boolean(process.argv[1]) && resolve(process.argv[1]) === self;

if (invokedDirectly) {
  const root = dirname(dirname(self));
  const flag = (name, fallback) => {
    const index = process.argv.indexOf(`--${name}`);
    if (index === -1) return fallback;
    const value = process.argv[index + 1];
    if (value === undefined || value.startsWith('--')) {
      console.error(`--${name} 需要一个值`);
      process.exit(2);
    }
    return value;
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
    { env: { ...process.env, DSH_HOME: process.env.DSH_HOME ?? join(process.env.HOME ?? '/', '.dsh') }, stdio: ['ignore', 'pipe', 'pipe'] }
  );

  // Keep the last lines of the runtime's stderr so a failure message can explain
  // *why* it died instead of merely reporting the exit code. Bounded to 8 lines
  // and 4096 bytes; everything is still forwarded live to process.stderr.
  const STDERR_TAIL_LINES = 8;
  const STDERR_TAIL_BYTES = 4096;
  let stderrTail = '';
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', (chunk) => {
    process.stderr.write(chunk);
    stderrTail = (stderrTail + chunk).split('\n').slice(-STDERR_TAIL_LINES).join('\n');
    if (Buffer.byteLength(stderrTail, 'utf8') > STDERR_TAIL_BYTES) {
      stderrTail = stderrTail.slice(-STDERR_TAIL_BYTES);
      const firstNewline = stderrTail.indexOf('\n');
      if (firstNewline !== -1) stderrTail = stderrTail.slice(firstNewline + 1);
    }
  });
  const stderrSuffix = () => stderrTail.trim() ? `; stderr tail: ${stderrTail.trim()}` : '';

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
    fail(`runtime did not print its URL within ${STARTUP_TIMEOUT}ms (startup timeout)${stderrSuffix()}`);
  }, STARTUP_TIMEOUT);

  let buffer = '';
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', async (chunk) => {
    buffer += chunk;
    const url = bannerUrl(buffer);
    if (!url || child.__checked || finished) return;
    child.__checked = true;
    console.log(`url ${url}`);

    try {
      const auth = await fetch(url, { redirect: 'manual', signal: AbortSignal.timeout(FETCH_TIMEOUT) });
      if (auth.status !== 303) return fail(`expected 303 from the token URL, got ${auth.status}`);
      const location = auth.headers.get('location');
      if (!location || !location.trim()) return fail('303 缺少 Location 头');
      // Location 可能携带 token：只说明「不在本地 origin」，绝不回显完整目标。
      if (!redirectIsLocal(location, url)) return fail('303 重定向目标不在本地 origin');
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
    if (!child.__checked) fail(`runtime exited before printing its URL (code ${code})${stderrSuffix()}`);
  });
}
