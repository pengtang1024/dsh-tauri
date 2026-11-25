import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

// Read the inline script from the source of truth so a page change cannot
// silently drift away from what these tests guarantee.
const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const html = readFileSync(join(projectRoot, 'src-tauri', 'dist', 'error.html'), 'utf8');
const scriptMatch = html.match(/<script>([\s\S]*?)<\/script>/);
assert.ok(scriptMatch, 'error.html must contain an inline <script> block');
const script = scriptMatch[1];

/**
 * Execute the page script in an isolated realm with a controlled URL hash.
 * The fake <pre> records every textContent/innerHTML write so the test can
 * assert both the rendered message and the absence of HTML injection.
 */
function runErrorPage(hash) {
  const writes = [];
  const detail = {
    set textContent(value) {
      writes.push({ prop: 'textContent', value });
    },
    set innerHTML(value) {
      writes.push({ prop: 'innerHTML', value });
    }
  };
  const document = {
    getElementById(id) {
      assert.equal(id, 'detail', 'page must only address the #detail element');
      return detail;
    }
  };
  const sandbox = {
    location: { hash },
    document,
    decodeURIComponent
  };
  vm.runInContext(script, vm.createContext(sandbox));
  return { writes, detail };
}

function renderedText(writes) {
  const texts = writes.filter((w) => w.prop === 'textContent');
  assert.equal(texts.length, 1, 'script must write textContent exactly once');
  return texts[0].value;
}

test('decodes a valid encoded message and writes it with textContent', () => {
  const message = '启动失败：npm 运行时未安装\n请手动执行：npm install\n符号：&<>"\'!';
  const { writes } = runErrorPage('#' + encodeURIComponent(message));
  assert.equal(renderedText(writes), message);
  assert.equal(writes.some((w) => w.prop === 'innerHTML'), false, 'no innerHTML write');
});

test('empty hash shows the unknown error fallback', () => {
  const { writes } = runErrorPage('');
  assert.equal(renderedText(writes), 'unknown error');
});

test('malformed percent escape does not throw and shows a readable fallback', () => {
  let result;
  assert.doesNotThrow(() => { result = runErrorPage('#100%done'); });
  const text = renderedText(result.writes);
  assert.match(text, /无法解析错误信息/);
  assert.match(text, /100%done/);
});

test('truncated UTF-8 percent escape does not throw and shows a readable fallback', () => {
  let result;
  assert.doesNotThrow(() => { result = runErrorPage('#%E0%A4%A'); });
  const text = renderedText(result.writes);
  assert.match(text, /无法解析错误信息/);
  assert.match(text, /%E0%A4%A/);
});

test('HTML-like message stays literal with no innerHTML write', () => {
  const message = '<img src=x onerror="alert(1)">危险';
  const { writes } = runErrorPage('#' + encodeURIComponent(message));
  assert.equal(renderedText(writes), message);
  assert.equal(writes.some((w) => w.prop === 'innerHTML'), false, 'no innerHTML write');
});

test('over-long malformed hash is truncated to 200 characters', () => {
  const raw = 'Z'.repeat(260) + '%';
  let result;
  assert.doesNotThrow(() => { result = runErrorPage('#' + raw); });
  const text = renderedText(result.writes);
  assert.match(text, /无法解析错误信息/);
  assert.ok(text.endsWith('Z'.repeat(200)), 'fallback must include the first 200 raw characters');
  assert.equal(text.includes('Z'.repeat(201)), false, 'fallback must not exceed 200 raw characters');
});
