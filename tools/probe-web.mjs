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
const runtime = process.env.DSH_TAURI_RUNTIME ?? join(root, 'runtime');
const entry = join(runtime, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js');
const profile = process.argv.includes('--profile') ? process.argv[process.argv.indexOf('--profile') + 1] : 'web';

if (!existsSync(entry)) {
  console.error(`missing runtime: ${entry}\n run: npm install --prefix runtime @deepseek-ai/dsh`);
  process.exit(2);
}

const child = spawn(
  process.env.DSH_TAURI_NODE ?? 'node',
  [entry, '--profile', profile, '--host', '127.0.0.1', '--port', '0', '--no-open'],
  { env: { ...process.env, DSH_HOME: process.env.DSH_HOME ?? join(process.env.HOME ?? '/', '.dsh') }, stdio: ['ignore', 'pipe', 'inherit'] }
);

const fail = (message) => {
  console.error(`FAIL ${message}`);
  child.kill('SIGTERM');
  process.exit(1);
};

let buffer = '';
child.stdout.setEncoding('utf8');
child.stdout.on('data', async (chunk) => {
  buffer += chunk;
  const match = buffer.match(/dsh web:\s+(http:\/\/127\.0\.0\.1:\d+\/\?token=[\w-]+)/);
  if (!match || child.__checked) return;
  child.__checked = true;
  const url = match[1];
  console.log(`url ${url}`);

  const auth = await fetch(url, { redirect: 'manual' });
  if (auth.status !== 303) fail(`expected 303 from the token URL, got ${auth.status}`);
  const cookie = auth.headers.get('set-cookie');
  if (!cookie) fail('the token URL answered without a session cookie');
  console.log(`auth 303 → ${cookie.split(';')[0].split('=')[0]}`);

  const html = await fetch(new URL(url).origin, { headers: { cookie: cookie.split(';')[0] } });
  const body = await html.text();
  if (html.status !== 200) fail(`expected 200 from /, got ${html.status}`);
  if (!body.includes('id="root"')) fail('the client HTML does not contain the app root');
  console.log(`root 200 ${body.length}B (client HTML)`);
  console.log('RESULT: OK — the runtime serves the client the shell embeds');
  child.kill('SIGTERM');
  process.exit(0);
});

child.on('exit', (code) => {
  if (!child.__checked) fail(`runtime exited before printing its URL (code ${code})`);
});
