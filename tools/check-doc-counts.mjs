#!/usr/bin/env node
/**
 * Read-only guard: the hard-coded Rust unit-test counts in three docs must
 * match the real number of #[test] attributes in the Rust sources under
 * src-tauri/src (recursively, all .rs files).
 *
 * Only node:fs / node:path / node:url are used; nothing is written.
 * Prints one PASS/FAIL per doc with file:line and expected/actual, then
 * "SUMMARY: 3 checks, M FAILS". Exits 1 when any check fails, else 0.
 */
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function* rustSources(dir) {
  let entries;
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) { yield* rustSources(path); continue; }
    if (entry.isFile() && entry.name.endsWith('.rs')) yield path;
  }
}

const testLine = /^\s*#\[test\]/;
let rustTestCount = 0;
for (const file of rustSources(join(root, 'src-tauri', 'src'))) {
  const text = readFileSync(file, 'utf8');
  for (const line of text.split(/\r?\n/)) if (testLine.test(line)) rustTestCount++;
}

const checks = [
  { name: 'README.md', file: 'README.md', pattern: /Rust 单测 (\d+) 项/ },
  { name: 'CHANGELOG.md', file: 'CHANGELOG.md', pattern: /从 17 个增至 (\d+) 个/ },
  { name: 'DESKTOP_BRIDGE.zh.md', file: 'docs/DESKTOP_BRIDGE.zh.md', pattern: /cargo test`?\s+(\d+) passed/ },
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
  if (found && Number(found.value) === rustTestCount) {
    console.log('PASS ' + check.name + ' — ' + where + ' — 期望 ' + rustTestCount + ' / 实际 ' + found.value);
  } else {
    const actual = found ? found.value : 'PATTERN NOT FOUND';
    console.log('FAIL ' + check.name + ' — ' + where + ' — 期望 ' + rustTestCount + ' / 实际 ' + actual);
    FAILS.push(check.name);
  }
}

console.log('SUMMARY: ' + checks.length + ' checks, ' + FAILS.length + ' FAILS');
if (FAILS.length > 0) {
  console.error('文档测试数量与 Rust 源码不一致（' + FAILS.join('、') + '）：请同步文档中的测试数量或修正统计（src-tauri/src/**/*.rs 实际 #[test] = ' + rustTestCount + '）。');
  process.exit(1);
}
