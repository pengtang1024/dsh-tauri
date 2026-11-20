import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { test } from 'node:test';

// shell-api resolves its paths at import time, so the fake home must be in place
// before the module is loaded.
const fakeHome = mkdtempSync(join(tmpdir(), 'dsh-tauri-test-'));
process.env.HOME = fakeHome;
const { handleShellRequest } = await import('../bridge/shell-api.mjs');

/** Minimal request/response pair: enough for the router under test. */
function call(method, path, body) {
  const payload = body === undefined ? '' : JSON.stringify(body);
  const request = Readable.from(payload === '' ? [] : [Buffer.from(payload)]);
  request.method = method;
  request.url = path;
  const captured = { status: 0, headers: {}, body: '' };
  const response = {
    writeHead(status, headers) {
      captured.status = status;
      captured.headers = headers;
    },
    end(chunk) {
      captured.body = chunk ?? '';
    }
  };
  return handleShellRequest(request, response).then((handled) => ({ handled, ...captured }));
}

test('stage-file copies a dropped file and returns a readable absolute path', async () => {
  const result = await call('POST', '/__dsh_tauri__/stage-file', {
    name: 'notes.txt',
    dataBase64: Buffer.from('hello staged').toString('base64')
  });
  assert.equal(result.status, 200);
  const payload = JSON.parse(result.body);
  assert.ok(payload.path.startsWith(fakeHome), payload.path);
  assert.equal(readFileSync(payload.path, 'utf8'), 'hello staged');
  assert.equal(payload.size, 12);
});

test('stage-file rejects an unusable name', async () => {
  const result = await call('POST', '/__dsh_tauri__/stage-file', { name: '..', dataBase64: 'aGk=' });
  assert.equal(result.status, 500);
  assert.match(JSON.parse(result.body).error, /usable name/);
});

test('hiding path traversal: basename is stripped from the staged name', async () => {
  const result = await call('POST', '/__dsh_tauri__/stage-file', {
    name: '../../etc/passwd',
    dataBase64: Buffer.from('x').toString('base64')
  });
  assert.equal(result.status, 200);
  const payload = JSON.parse(result.body);
  assert.ok(payload.path.includes('dsh-tauri-drops'), payload.path);
  assert.ok(!payload.path.includes('..'), payload.path);
});

test('every shell route answers with JSON and no-store', async () => {
  const result = await call('GET', '/__dsh_tauri__/info');
  assert.equal(result.status, 200);
  assert.equal(result.headers['cache-control'], 'no-store');
  assert.equal(typeof JSON.parse(result.body).platform, 'string');
});

test('unknown routes are rejected rather than forwarded', async () => {
  const result = await call('GET', '/__dsh_tauri__/nope');
  assert.equal(result.status, 404);
  assert.equal(result.handled, true);
});

test('the shell API ignores paths outside its prefix', async () => {
  const result = await call('GET', '/api/health');
  assert.equal(result.handled, false);
});

test.after(() => rmSync(fakeHome, { recursive: true, force: true }));
