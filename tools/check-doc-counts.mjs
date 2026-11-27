#!/usr/bin/env node
/**
 * Read-only guard: hard-coded test counts in docs must match the real sources:
 * Rust unit tests (#[test] attributes under src-tauri/src, recursively) and
 * npm unit tests (node:test test() calls in .test.mjs files under test/,
 * recursively, strict line-start match so helpers/comments are not counted).
 * It also checks the bridge contract guard's check count (the GUARD_CHECK_COUNT
 * constant in tools/check-bridge-contract.mjs) against the counts documented in
 * CHANGELOG.md, README.md and docs/DESKTOP_BRIDGE.zh.md, without running the
 * guard (which scans thousands of files). It also guards the remembered window
 * geometry contract: the two literals in shell_ui.rs::state_path() must stay
 * ".dsh" and "dsh-tauri-window-state.json", and the documented path must agree
 * in README.md and all seven docs/DETAIL.<lang>.md files, without touching the
 * filesystem, HOME or any environment variable.
 *
 * Only node:fs / node:path / node:url are used; nothing is written.
 * Prints one PASS/FAIL per check with file:line and expected/actual, then
 * "SUMMARY: 19 checks, M FAILS". Exits 1 when any check fails, else 0.
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

const shimAssertPattern = /\bassert\.[A-Za-z0-9_]+\(/g;
const shimContractText = readFileSync(join(root, 'test', 'shim-contract.test.mjs'), 'utf8');
const shimAssertCount = (shimContractText.match(shimAssertPattern) || []).length; // 预期 21

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
  { name: 'README.md 契约守卫检查项数（' + guardPath + ':' + guardLine + '）', file: 'README.md', pattern: /上游桥接漂移守卫，(\d+) 项静态检查/, expected: guardCount },
  { name: 'CHANGELOG.md npm 单测', file: 'CHANGELOG.md', pattern: /npm 单测总数达 (\d+) 项/, expected: npmTestCount },
  { name: 'CHANGELOG.md SHIM 契约断言数', file: 'CHANGELOG.md', pattern: /SHIM 契约单测（(\d+) 项断言）/, expected: shimAssertCount },
  { name: 'DESKTOP_BRIDGE.zh.md 契约守卫检查项数（' + guardPath + ':' + guardLine + '）', file: 'docs/DESKTOP_BRIDGE.zh.md', pattern: /契约守卫 (\d+) checks 0 FAIL/, expected: guardCount },
];

// --- window state path contract: read source text only; no fs/env probing ---
const shellUiPath = 'src-tauri/src/shell_ui.rs';
const shellUiText = readFileSync(join(root, shellUiPath), 'utf8');
const statePathPattern = /fn\s+state_path\s*\(\s*\)\s*->\s*PathBuf\s*\{[\s\S]*?\}/;
const statePathMatch = shellUiText.match(statePathPattern);
const statePathBody = statePathMatch ? statePathMatch[0] : '';
const statePathLine = statePathMatch
  ? shellUiText.slice(0, shellUiText.indexOf(statePathMatch[0])).split(/\r?\n/).length
  : '?';
const expectedDirLiteral = '".dsh"';
const expectedFileLiteral = '"dsh-tauri-window-state.json"';
const documentedPath = '~/.dsh/dsh-tauri-window-state.json';

checks.push({
  name: 'shell_ui.rs::state_path() 目录字面量（' + shellUiPath + ':' + statePathLine + '）',
  file: shellUiPath,
  pattern: /fn\s+state_path\s*\(\s*\)\s*->\s*PathBuf\s*\{[\s\S]*?\.join\("([^"]*)"\)[\s\S]*?\}/,
  custom: () => (statePathBody.match(/\.join\("([^"]*)"\)/g) || []).some((s) => s === '.join(' + expectedDirLiteral + ')'),
  expected: expectedDirLiteral,
  actual: (statePathBody.match(/\.join\("([^"]*)"\)/g) || []).join('、') || 'PATTERN NOT FOUND',
  where: shellUiPath + ':' + statePathLine,
});
checks.push({
  name: 'shell_ui.rs::state_path() 文件名字面量（' + shellUiPath + ':' + statePathLine + '）',
  file: shellUiPath,
  pattern: /fn\s+state_path\s*\(\s*\)\s*->\s*PathBuf\s*\{[\s\S]*?\}/,
  custom: () => statePathBody.includes(expectedFileLiteral),
  expected: expectedFileLiteral,
  actual: (statePathBody.match(/\.join\("([^"]*)"\)/g) || []).join('、') || 'PATTERN NOT FOUND',
  where: shellUiPath + ':' + statePathLine,
});
for (const file of ['README.md', 'docs/DETAIL.zh.md', 'docs/DETAIL.en.md', 'docs/DETAIL.es.md',
  'docs/DETAIL.fr.md', 'docs/DETAIL.de.md', 'docs/DETAIL.ru.md', 'docs/DETAIL.pt.md']) {
  const docText = readFileSync(join(root, file), 'utf8');
  const docLine = docText.split(/\r?\n/).findIndex((line) => line.includes(documentedPath)) + 1;
  checks.push({
    name: file + ' 窗口状态路径文档',
    file,
    pattern: /~\/\.dsh\/dsh-tauri-window-state\.json/,
    expected: documentedPath,
    custom: () => docLine > 0,
    actual: docLine > 0 ? documentedPath : 'PATTERN NOT FOUND',
    where: file + ':' + (docLine || '?'),
  });
}

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

  const where = check.custom ? check.where : (found ? check.file + ':' + found.line : check.file + ':?');
  const ok = check.custom
    ? check.custom()
    : (found !== null && Number(found.value) === check.expected);
  const actual = check.custom ? check.actual : (found ? found.value : 'PATTERN NOT FOUND');
  if (ok) {
    console.log('PASS ' + check.name + ' — ' + where + ' — 期望 ' + check.expected + ' / 实际 ' + actual);
  } else {
    console.log('FAIL ' + check.name + ' — ' + where + ' — 期望 ' + check.expected + ' / 实际 ' + actual);
    FAILS.push(check.name);
  }
}

console.log('SUMMARY: ' + checks.length + ' checks, ' + FAILS.length + ' FAILS');
if (FAILS.length > 0) {
  console.error('文档声明与源码/窗口状态路径契约不一致（' + FAILS.join('、') + '）：请同步文档或源码（src-tauri/src/**/*.rs 实际 #[test] = ' + rustTestCount + '；test/**/*.test.mjs 实际 test() = ' + npmTestCount + '）。');
  process.exit(1);
}
