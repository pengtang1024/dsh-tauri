import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

// The shim is a Rust raw string injected by native.rs before any page code.
// Read it from the source of truth so a contract change cannot silently drift.
const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const nativeSource = readFileSync(join(projectRoot, 'src-tauri', 'src', 'native.rs'), 'utf8');
const shimMatch = nativeSource.match(/pub const SHIM:\s*&str\s*=\s*r#"([\s\S]*?)"#\s*;/);
assert.ok(shimMatch, 'native.rs must define SHIM as a raw string');
const shim = shimMatch[1];

/**
 * Execute the injected shim in an isolated realm with fake native/dom hosts.
 * Every core.invoke call is recorded synchronously for exact-contract checks.
 */
function loadShim({ withBridge = true, title = '', readyState = 'complete', nativeOpen } = {}) {
  const calls = [];
  const listeners = {};
  const head = { tagName: 'HEAD' };
  const documentElement = { tagName: 'HTML' };
  const document = {
    title,
    head,
    documentElement,
    readyState,
    addEventListener(name, callback) {
      (listeners[name] ||= []).push(callback);
    }
  };
  let observer = null;
  class FakeMutationObserver {
    constructor(callback) {
      this.callback = callback;
      observer = this;
    }
    observe(target, options) {
      this.target = target;
      this.options = { ...options };
    }
  }
  const nativeOpenCalls = [];
  const sandbox = {
    document,
    MutationObserver: FakeMutationObserver,
    open: (...args) => {
      nativeOpenCalls.push(args);
      return nativeOpen === undefined ? null : nativeOpen(...args);
    }
  };
  if (withBridge) {
    sandbox.__TAURI__ = {
      core: {
        // Copy into the host realm: vm-created objects have a different
        // prototype, which deepStrictEqual treats as non-identical.
        invoke(command, args) {
          calls.push({ command, args: args ? { ...args } : args });
          return Promise.resolve(undefined);
        }
      }
    };
  }
  vm.runInContext(shim, vm.createContext(sandbox));
  return { sandbox, document, calls, listeners, observer, head, documentElement, nativeOpenCalls };
}

test('injects __DSH_TAURI__ v1 and routes all APIs through core.invoke', async () => {
  const { sandbox, calls } = loadShim();
  const api = sandbox.__DSH_TAURI__;

  assert.ok(api, '__DSH_TAURI__ must be injected');
  assert.equal(api.protocolVersion, 1);

  await api.info();
  await api.openExternal('https://example.com/docs');
  await api.notify('Title', 'Body');
  await api.clipboard.readText();
  await api.clipboard.writeText('payload');

  assert.deepEqual(calls, [
    { command: 'shell_info', args: undefined },
    { command: 'open_external', args: { url: 'https://example.com/docs' } },
    { command: 'notify', args: { title: 'Title', body: 'Body' } },
    { command: 'clipboard_read', args: undefined },
    { command: 'clipboard_write', args: { text: 'payload' } }
  ]);
});

test('directory picker calls pick_directory and forwards options.title', async () => {
  const { sandbox, calls } = loadShim();
  const picker = sandbox.__DSH_DIRECTORY_PICKER__;

  assert.ok(picker, '__DSH_DIRECTORY_PICKER__ must be injected');
  await picker.pick({ title: '选择项目目录' });
  await picker.pick();

  assert.deepEqual(calls, [
    { command: 'pick_directory', args: { title: '选择项目目录' } },
    { command: 'pick_directory', args: { title: undefined } }
  ]);
});

test('mirrors document.title into set_title but ignores empty titles', () => {
  const { document, calls, observer, listeners, head, documentElement } = loadShim({ title: 'Initial' });

  assert.ok(observer, 'title sync must install a MutationObserver');
  assert.equal(observer.target, head);
  assert.deepEqual(observer.options, { subtree: true, childList: true, characterData: true });
  assert.deepEqual(listeners, {}, 'a ready document starts synchronously');
  assert.deepEqual(calls, [{ command: 'set_title', args: { title: 'Initial' } }]);

  observer.callback();
  assert.equal(calls.length, 1, 'an unchanged title must not call set_title again');

  document.title = '';
  observer.callback();
  assert.equal(calls.length, 1, 'an empty title must not call set_title');

  document.title = 'Renamed';
  observer.callback();
  assert.deepEqual(calls.at(-1), { command: 'set_title', args: { title: 'Renamed' } });

  assert.ok(documentElement, 'document.documentElement must remain available as fallback');
});

test('defers title sync until DOMContentLoaded while the document is loading', () => {
  const { calls, observer, listeners } = loadShim({ title: 'Deferred', readyState: 'loading' });

  assert.equal(observer, null);
  assert.equal(calls.length, 0);
  assert.equal(listeners.DOMContentLoaded.length, 1);
  listeners.DOMContentLoaded[0]();
  assert.deepEqual(calls, [{ command: 'set_title', args: { title: 'Deferred' } }]);
});

test('API calls reject when the Tauri bridge is missing', async () => {
  const { sandbox } = loadShim({ withBridge: false });

  await assert.rejects(sandbox.__DSH_TAURI__.openExternal('https://example.com'), /Tauri bridge unavailable/);
  await assert.rejects(sandbox.__DSH_DIRECTORY_PICKER__.pick({ title: 'x' }), /Tauri bridge unavailable/);
});

test('login popup: about:blank + _blank returns a stub and replace routes to open_external', async () => {
  const { sandbox, calls, nativeOpenCalls } = loadShim();

  // Exact upstream shape from dsh-client-ui-settings-account browser-login.ts:
  // window.open('about:blank', '_blank'); popup.opener = null;
  // popup.location.replace(destination).
  const popup = sandbox.open('about:blank', '_blank');
  assert.ok(popup, 'the shim must return a non-null stub popup');
  assert.equal(popup.closed, false);
  popup.opener = null;
  assert.equal(popup.opener, null, 'opener must be writable and clearable');

  await popup.location.replace('https://auth.example.com/auth.html?theme=dark');
  assert.deepEqual(calls, [
    { command: 'open_external', args: { url: 'https://auth.example.com/auth.html?theme=dark' } }
  ]);
  assert.deepEqual(nativeOpenCalls, [], 'the intercepted shape must not call the native open');

  popup.close();
  assert.equal(popup.closed, true, 'close() must flip closed to true');
});

test('login popup: other window.open shapes keep native behavior', () => {
  const sentinel = { tag: 'native-window' };
  const { sandbox, calls, nativeOpenCalls } = loadShim({ nativeOpen: () => sentinel });

  assert.equal(sandbox.open('https://example.com', '_blank'), sentinel);
  assert.equal(sandbox.open('about:blank', '_self'), sentinel);
  assert.equal(sandbox.open('about:blank'), sentinel);
  assert.deepEqual(nativeOpenCalls, [
    ['https://example.com', '_blank'],
    ['about:blank', '_self'],
    ['about:blank']
  ]);
  assert.deepEqual(calls, [], 'non-login shapes must never invoke open_external');
});

test('login popup: no dshDesktop global and no data-platform write in SHIM', () => {
  const { sandbox } = loadShim();
  assert.equal(sandbox.dshDesktop, undefined, 'SHIM must not inject dshDesktop');
  assert.ok(!shim.includes('dshDesktop'), 'SHIM source must not mention dshDesktop');
  assert.ok(!shim.includes('data-platform'), 'SHIM source must not set data-platform');
});
