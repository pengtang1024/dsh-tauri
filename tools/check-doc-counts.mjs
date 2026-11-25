#!/usr/bin/env node
/**
 * Read-only guard: hard-coded test counts in docs must match the real sources:
 * Rust unit tests (#[test] attributes under src-tauri/src, recursively) and
 * npm unit tests (node:test test() calls in .test.mjs files under test/,
 * recursively, strict line-start match so helpers/comments are not counted).
 * It also checks the bridge contract guard's check count (the GUARD_CHECK_COUNT
 * constant in tools/check-bridge-contract.mjs) against the count documented in
 * CHANGELOG.md, without running the guard (which scans thousands of files).
 *
 * Only node:fs / node:path / node:url are used; nothing is written.
 * Prints one PASS/FAIL per check with file:line and expected/actual, then
 * "SUMMARY: 5 checks, M FAILS". Exits 1 when any check fails, else 0.
 */
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function* sourceFiles(dir, suffix) {
  let entries;
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) { yield* sourceFiles(path, suffix); continue; }
    if (entry.isFile() && entry.name.endsWith(suffix)) yield path;
  }
}

const rustTestLine = /^\s*#\[test\]/;
let rustTestCount = 0;
for (const file of sourceFiles(join(root, 'src-tauri', 'src'), '.rs')) {
  const text = readFileSync(file, 'utf8');
  for (const line of text.split(/\r?\n/)) if (rustTestLine.test(line)) rustTestCount++;
}

const npmTestLine = /^\s*test\s*\(/;
let npmTestCount = 0;
for (const file of sourceFiles(join(root, 'test'), '.test.mjs')) {
  const text = readFileSync(file, 'utf8');
  for (const line of text.split(/\r?\n/)) if (npmTestLine.test(line)) npmTestCount++;
}

const guardPath = 'tools/check-bridge-contract.mjs';
let guardCount = null;
let guardLine = '?';
const guardText = readFileSync(join(root, guardPath), 'utf8');
for (const [i, line] of guardText.split(/\r?\n/).entries()) {
  const m = line.match(/GUARD_CHECK_COUNT\s*=\s*(\d+)/);
  if (m) { guardCount = Number(m[1]); guardLine = i + 1; break; }
}

const checks = [
  { name: 'README.md', file: 'README.md', pattern: /Rust 单测 (\d+) 项/, expected: rustTestCount },
  { name: 'CHANGELOG.md', file: 'CHANGELOG.md', pattern: /从 17 个增至 (\d+) 个/, expected: rustTestCount },
  { name: 'DESKTOP_BRIDGE.zh.md', file: 'docs/DESKTOP_BRIDGE.zh.md', pattern: /cargo test`?\s+(\d+) passed/, expected: rustTestCount },
  { name: 'README.md npm 单测', file: 'README.md', pattern: /注入脚本、CLI、错误页与启动页单测，(\d+) 项/, expected: npmTestCount },
  { name: 'CHANGELOG.md 契约守卫检查项数（' + guardPath + ':' + guardLine + '）', file: 'CHANGELOG.md', pattern: /契约漂移守卫.*?静态检查（(\d+) 项/, expected: guardCount },
];

const FAILS = [];
for (const check of checks) {
  const path = join(root, check.file);
  let text;
  try { text = readFileSync(path, 'utf8'); } catch { text = null; }

  let found = null;
  if (text !== null) {
    const lines = text.split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      const m = lines[i].match(check.pattern);
      if (m) { found = { value: m[1], line: i + 1 }; break; }
    }
  }

  const where = found ? check.file + ':' + found.line : check.file + ':?';
  if (found && Number(found.value) === check.expected) {
    console.log('PASS ' + check.name + ' — ' + where + ' — 期望 ' + check.expected + ' / 实际 ' + found.value);
  } else {
    const actual = found ? found.value : 'PATTERN NOT FOUND';
    console.log('FAIL ' + check.name + ' — ' + where + ' — 期望 ' + check.expected + ' / 实际 ' + actual);
    FAILS.push(check.name);
  }
}

console.log('SUMMARY: ' + checks.length + ' checks, ' + FAILS.length + ' FAILS');
if (FAILS.length > 0) {
  console.error('文档测试数量与源码不一致（' + FAILS.join('、') + '）：请同步文档中的测试数量或修正统计（src-tauri/src/**/*.rs 实际 #[test] = ' + rustTestCount + '；test/**/*.test.mjs 实际 test() = ' + npmTestCount + '）。');
  process.exit(1);
}
