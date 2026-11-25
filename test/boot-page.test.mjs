import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

// Read the inline script from the source of truth so a page change cannot
// silently drift away from what these tests guarantee.
const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const html = readFileSync(join(projectRoot, 'src-tauri', 'dist', 'boot.html'), 'utf8');
const scriptMatch = html.match(/<script>([\s\S]*?)<\/script>/);
assert.ok(scriptMatch, 'boot.html must contain an inline <script> block');
const script = scriptMatch[1];

/**
 * Execute the boot script in an isolated realm with a fake DOM and Tauri
 * bridge. The fake timer records the callback without ever firing it; the
 * fake invoke records each call and hands back a manually resolvable promise.
 * That lets the tests simulate a response arriving after several ticks.
 */
function loadBootPage({ withBridge = true } = {}) {
  const elements = {
    stage: makeElement('stage'),
    hint: makeElement('hint'),
    spinner: makeElement('spinner')
  };
  const document = {
    getElementById(id) {
      assert.ok(Object.hasOwn(elements, id), 'page must only address known elements');
      return elements[id];
    }
  };
  const calls = [];
  const pending = [];
  const timers = [];
  let cleared = 0;
  const sandbox = {
    document,
    console,
    setInterval(callback, delay) {
      timers.push({ callback, delay });
      return timers.length; // a truthy handle, like a real timer id
    },
    clearInterval(handle) {
      assert.equal(handle, 1, 'clearInterval must receive the timer handle');
      cleared += 1;
    }
  };
  if (withBridge) {
    sandbox.__TAURI__ = {
      core: {
        invoke(command) {
          assert.equal(command, 'runtime_status');
          let resolve;
          let reject;
          const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
          pending.push({ resolve, reject });
          calls.push(command);
          return promise;
        }
      }
    };
  }
  vm.runInContext(script, vm.createContext(sandbox));
  return {
    elements, calls, pending, timers,
    tick() {
      assert.equal(timers.length, 1, 'page must register exactly one interval');
      timers[0].callback();
    },
    get cleared() { return cleared; }
  };
}

function makeElement(id) {
  const writes = [];
  return {
    id,
    writes,
    textContent: '',
    style: {},
    set innerHTML(value) {
      writes.push({ prop: 'innerHTML', value });
    }
  };
}

test('page uses no innerHTML or eval', () => {
  assert.equal(/\binnerHTML\b/.test(script), false, 'no innerHTML write');
  assert.equal(/\beval\s*\(/.test(script), false, 'no eval call');
});

test('polls once immediately and registers a 500ms interval', () => {
  const page = loadBootPage();
  assert.deepEqual(page.calls, ['runtime_status'], 'first frame must poll immediately');
  assert.equal(page.timers.length, 1);
  assert.equal(page.timers[0].delay, 500);
});

test('a tick while a request is in flight does not start a second poll', () => {
  const page = loadBootPage();
  assert.equal(page.pending.length, 1);
  page.tick();
  assert.equal(page.calls.length, 1, 'in-flight tick must be skipped');
  assert.equal(page.pending.length, 1);
});

test('after the first response resolves the next tick polls again', async () => {
  const page = loadBootPage();
  page.tick();
  page.pending[0].resolve({ stage: 'locating', elapsedSecs: 1 });
  await new Promise((resolve) => setImmediate(resolve)); // drain .then/.finally microtasks
  assert.equal(page.calls.length, 1, 'resolution alone must not trigger a poll');
  page.tick();
  assert.deepEqual(page.calls, ['runtime_status', 'runtime_status']);
  assert.equal(page.pending.length, 2);
});

test('a failed invoke shows the shell communication fallback and unlocks the next tick', async () => {
  const page = loadBootPage();
  page.pending[0].reject(new Error('shell busy'));
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(page.elements.hint.textContent, '与外壳通信失败，请重启应用');
  page.tick();
  assert.equal(page.calls.length, 2, 'rejection must also clear inFlight');
});

test('a valid status writes the mapped hint and elapsed seconds with textContent', async () => {
  const page = loadBootPage();
  page.pending[0].resolve({ stage: 'installing', message: '安装运行时', elapsedSecs: 12 });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(page.elements.stage.textContent, '安装运行时（已用 12 秒）');
  assert.equal(page.elements.hint.textContent,
    '首次启动会自动安装 @deepseek-ai/dsh，约需 1–2 分钟，请不要关闭窗口');
  assert.equal(page.elements.stage.textContent.includes('12'), true);
  for (const el of Object.values(page.elements)) {
    assert.equal(el.writes.some((w) => w.prop === 'innerHTML'), false, 'no innerHTML write');
  }
});

test('error is terminal: shows error text, hides spinner, clears timer and stops polling', async () => {
  const page = loadBootPage();
  page.pending[0].resolve({ stage: 'error', error: '启动失败：端口被占用', elapsedSecs: 3 });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(page.elements.stage.textContent, 'DSH 启动失败');
  assert.equal(page.elements.hint.textContent, '启动失败：端口被占用');
  assert.equal(page.elements.spinner.style.display, 'none');
  assert.equal(page.cleared, 1, 'interval must be cleared on error');
  page.tick();
  page.tick();
  assert.equal(page.calls.length, 1, 'no further invoke after terminal error');
});

test('error message falls back to message then to a generic string', async () => {
  const withMessage = loadBootPage();
  withMessage.pending[0].resolve({ stage: 'error', message: '备用错误信息' });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(withMessage.elements.hint.textContent, '备用错误信息');

  const generic = loadBootPage();
  generic.pending[0].resolve({ stage: 'error' });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(generic.elements.hint.textContent, '未知错误');
});

test('an in-flight error response also prevents a queued tick from polling again', async () => {
  const page = loadBootPage();
  page.tick(); // skipped while first request is in flight
  page.pending[0].resolve({ stage: 'error', error: 'boom' });
  await new Promise((resolve) => setImmediate(resolve));
  page.tick();
  assert.equal(page.calls.length, 1, 'done flag must survive later ticks');
  assert.equal(page.cleared, 1);
});

test('a missing Tauri bridge polls nothing', () => {
  const page = loadBootPage({ withBridge: false });
  page.tick();
  assert.deepEqual(page.calls, []);
});
