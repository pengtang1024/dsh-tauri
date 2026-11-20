/**
 * Native surface exposed to the web application through the loopback origin.
 *
 * The renderer runs on `http://127.0.0.1:<port>` — the bridge's proxy — so it
 * cannot use Tauri's IPC directly (that is bound to the Tauri origin). Instead
 * the shell exposes a small same-origin HTTP API under `/__dsh_tauri__/`, which
 * `bridge/renderer-shim.js` turns into the `window.dshDesktop*` globals that the
 * Electron preload used to provide.
 *
 * Every handler is deliberately small: a capability earns its place only when a
 * loaded client plugin reads it.
 */
import { execFile } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { basename, join } from 'node:path';

const BASE = '/__dsh_tauri__';

/** Sink for events that must reach the Rust side; the bridge relays them. */
let emitEvent = () => {};

/** @param sink - receives `{type, ...}` objects for the shell's stdout channel. */
export function setEventSink(sink) {
  emitEvent = sink;
}
const PREFERENCES_FILE = join(homedir(), '.dsh', 'desktop-preferences.json');
/** Dropped files are copied here so the Host can read them by absolute path. */
const DROP_DIR = join(homedir(), '.dsh', 'dsh-tauri-drops');
/** Refuse blobs above this size: staging is a copy, not a streaming upload. */
const MAX_STAGE_BYTES = 256 * 1024 * 1024;

/** Run a GUI helper, resolving with its trimmed stdout; rejects with stderr. */
function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    execFile(command, args, { timeout: 120_000, ...options }, (error, stdout, stderr) => {
      if (error) {
        reject(new Error(stderr.trim() || error.message));
        return;
      }
      resolve(stdout.trim());
    });
  });
}

function readPreferences() {
  try {
    return JSON.parse(readFileSync(PREFERENCES_FILE, 'utf8'));
  } catch {
    return {};
  }
}

function writePreferences(patch) {
  const merged = { ...readPreferences(), ...patch };
  writeFileSync(PREFERENCES_FILE, JSON.stringify(merged, null, 2));
  return merged;
}

const routes = {
  'GET /info': async () => ({
    platform: process.platform,
    profile: process.env.DSH_TAURI_PROFILE ?? null,
    preferences: readPreferences()
  }),

  'GET /preferences': async () => readPreferences(),

  'POST /preferences': async (body) => writePreferences(body?.patch ?? {}),

  /** Native directory chooser; resolves to `{ path: null }` when cancelled. */
  'POST /pick-directory': async (body) => {
    const title = typeof body?.title === 'string' ? body.title : '选择文件夹';
    try {
      const path = await run('zenity', [
        '--file-selection',
        '--directory',
        '--title',
        title,
        ...(typeof body?.defaultPath === 'string' && body.defaultPath !== '' ? [`--filename=${body.defaultPath}/`] : [])
      ]);
      return { path: path === '' ? null : path };
    } catch (error) {
      if (String(error.message).includes('cancel')) return { path: null };
      throw error;
    }
  },

  /**
   * Copy a dropped file into the drop directory and return its absolute path.
   * The renderer cannot learn real paths from a `File`, so the shim stages the
   * bytes here and remembers the mapping; this is what makes attaching a local
   * file work without the Electron `webUtils.getPathForFile` helper.
   */
  'POST /stage-file': async (body) => {
    const raw = typeof body?.name === 'string' ? body.name : '';
    const name = basename(raw).replace(/[^\w.\- ]+/g, '_').slice(0, 120);
    if (name === '' || name.startsWith('.')) throw new Error('refusing to stage a file without a usable name');
    const data = typeof body?.dataBase64 === 'string' ? Buffer.from(body.dataBase64, 'base64') : Buffer.alloc(0);
    if (data.length === 0) throw new Error('staged file is empty');
    if (data.length > MAX_STAGE_BYTES) throw new Error('staged file is too large');
    const target = join(DROP_DIR, randomUUID(), name);
    mkdirSync(join(target, '..'), { recursive: true });
    writeFileSync(target, data);
    return { path: target, size: statSync(target).size };
  },

  'POST /open-external': async (body) => {
    const url = new URL(String(body?.url ?? ''));
    if (url.protocol !== 'https:' && url.protocol !== 'http:') throw new Error('only http(s) links can be opened');
    await run('xdg-open', [url.toString()], { timeout: 10_000 }).catch(() => undefined);
    return { opened: true };
  },

  'POST /clipboard': async (body) => {
    if (typeof body?.text === 'string') {
      await new Promise((resolve, reject) => {
        const child = execFile('wl-copy', [], { timeout: 10_000 }, (error) => (error ? reject(error) : resolve()));
        child.stdin?.end(body.text);
      });
      return { text: body.text };
    }
    return { text: await run('wl-paste', ['--no-newline']) };
  },

  'POST /notify': async (body) => {
    await run('notify-send', ['-a', 'DSH', String(body?.title ?? 'DSH'), String(body?.body ?? '')], { timeout: 10_000 }).catch(
      () => undefined
    );
    return { shown: true };
  },

  /** Best-effort probe of the OS permission grants the Electron shell reported. */
  'GET /permissions': async () => ({
    microphone: await run('bash', ['-lc', "pactl get-default-source >/dev/null 2>&1 && echo granted || echo unknown"], { timeout: 5_000 })
      .then((out) => (out === 'granted' ? 'granted' : 'unknown'))
      .catch(() => 'unknown'),
    screen: 'granted',
    accessibility: 'unknown'
  }),

  /** Mirror the document title into the native window title. */
  'POST /window-title': async (body) => {
    const title = typeof body?.title === 'string' ? body.title.slice(0, 200) : '';
    emitEvent({ type: 'title', title });
    return { title };
  },

  'POST /report': async (body) => {
    process.stderr.write(`[renderer] ${JSON.stringify(body)}\n`);
    return { ok: true };
  }
};

/** Route one `/__dsh_tauri__/` request; returns true when it was handled. */
export async function handleShellRequest(request, response) {
  const url = new URL(request.url ?? '/', 'http://127.0.0.1');
  if (!url.pathname.startsWith(`${BASE}/`)) return false;

  const route = routes[`${request.method} ${url.pathname.slice(BASE.length)}`];
  if (route === undefined) {
    response.writeHead(404, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ error: `unknown shell route ${request.method} ${url.pathname}` }));
    return true;
  }

  let raw = '';
  for await (const chunk of request) raw += chunk;
  let body;
  if (raw !== '') {
    try {
      body = JSON.parse(raw);
    } catch (error) {
      response.writeHead(400, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ error: `invalid JSON body: ${error.message}` }));
      return true;
    }
  }

  try {
    const result = await route(body);
    response.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' });
    response.end(JSON.stringify(result ?? {}));
  } catch (error) {
    response.writeHead(500, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ error: String(error.message ?? error) }));
  }
  return true;
}
