#!/usr/bin/env node
/**
 * Read-only guard: capabilities/default.json must stay in sync with the labels
 * the Rust shell actually builds, and keep the core:default permission that
 * lets the SHIM call core.invoke. Without it a window-label or permission
 * drift silently revokes every native capability (ACL denial at runtime).
 *
 * Checks:
 * 1. capabilities/default.json exists and is valid JSON.
 * 2. windows is a non-empty array of non-empty strings.
 * 3. the windows set equals exactly the set of labels passed as the second
 *    argument of WebviewWindowBuilder::new(app, "<label>", ...) in
 *    src-tauri/src/**\/*.rs (recursive; duplicates in Rust collapse to a set).
 * 4. permissions is a non-empty array containing "core:default".
 *
 * Only node:fs / node:path / node:url are used; nothing is written.
 * Prints one PASS/FAIL per check with file:line, then
 * "SUMMARY: N checks, M FAILS". Exits 1 when any check fails, else 0.
 */
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const capsPath = join(root, 'src-tauri', 'capabilities', 'default.json');
const capsRel = relative(root, capsPath);
const srcDir = join(root, 'src-tauri', 'src');

function* sourceFiles(dir) {
  let entries;
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) { yield* sourceFiles(path); continue; }
    if (entry.isFile() && entry.name.endsWith('.rs')) yield path;
  }
}

const FAILS = [];
function pass(name, detail) { console.log('PASS ' + name + ' — ' + detail); }
function fail(name, detail) { console.log('FAIL ' + name + ' — ' + detail); FAILS.push(name); }
function lineOf(text, index) { return text.slice(0, index).split(/\r?\n/).length; }
function formatSet(values) { return values.length ? values.join('、') : '(空)'; }

// --- 1. file exists + valid JSON ---
let capsText = null;
let caps = null;
try {
  capsText = readFileSync(capsPath, 'utf8');
  pass('capabilities 文件存在', capsRel);
} catch {
  fail('capabilities 文件存在', capsRel + ' 不存在或无法读取');
}
if (capsText !== null) {
  try {
    caps = JSON.parse(capsText);
    pass('capabilities JSON 合法', capsRel);
  } catch (error) {
    fail('capabilities JSON 合法', capsRel + ' — ' + error.message);
  }
}

// --- 2. windows shape ---
const windows = caps && Array.isArray(caps.windows) ? caps.windows : null;
if (windows && windows.length > 0 && windows.every((w) => typeof w === 'string' && w.length > 0)) {
  pass('windows 字段非空字符串数组', capsRel + ' — ' + formatSet(windows));
} else {
  fail('windows 字段非空字符串数组', capsRel + ' — windows 缺失/为空/含非法值');
}

// --- 3. windows set == Rust WebviewWindowBuilder labels ---
const builderPattern = /\bWebviewWindowBuilder\s*::\s*new\s*\(\s*[A-Za-z_][A-Za-z0-9_:]*\s*,\s*(['"])([A-Za-z_][A-Za-z0-9_-]*)\1\s*,/g;
const rustLabels = new Map();
for (const file of sourceFiles(srcDir)) {
  const text = readFileSync(file, 'utf8');
  for (const m of text.matchAll(builderPattern)) {
    if (!rustLabels.has(m[2])) rustLabels.set(m[2], relative(root, file) + ':' + lineOf(text, m.index));
  }
}
const rustSet = [...rustLabels.keys()].sort((a, b) => a.localeCompare(b));
if (rustSet.length > 0) {
  pass('Rust 源码提取窗口标签', formatSet(rustSet.map((label) => label + '(' + rustLabels.get(label) + ')')));
} else {
  fail('Rust 源码提取窗口标签', '未从 src-tauri/src/**\/*.rs 提取到任何 WebviewWindowBuilder::new 标签');
}
if (windows && rustSet.length > 0) {
  const capsSet = [...new Set(windows)].sort((a, b) => a.localeCompare(b));
  const capsOnly = capsSet.filter((w) => !rustLabels.has(w));
  const rustOnly = rustSet.filter((w) => !capsSet.includes(w));
  if (capsOnly.length === 0 && rustOnly.length === 0) {
    pass('windows 集合与 Rust 窗口标签一致', capsRel + ' — ' + formatSet(capsSet));
  } else {
    fail('windows 集合与 Rust 窗口标签一致',
      '仅 capabilities: ' + formatSet(capsOnly) + '；仅 Rust: ' + formatSet(rustOnly));
  }
} else {
  fail('windows 集合与 Rust 窗口标签一致', 'windows 或 Rust 标签集合无效，无法比对');
}

// --- 4. permissions contains core:default ---
const permissions = caps && Array.isArray(caps.permissions) ? caps.permissions : null;
if (permissions && permissions.length > 0 && permissions.every((p) => typeof p === 'string' && p.length > 0)) {
  pass('permissions 字段非空字符串数组', capsRel + ' — ' + formatSet(permissions));
} else {
  fail('permissions 字段非空字符串数组', capsRel + ' — permissions 缺失/为空/含非法值');
}
if (permissions && permissions.includes('core:default')) {
  pass('permissions 含 core:default', capsRel + ' — SHIM 的 core.invoke 权限在位');
} else {
  fail('permissions 含 core:default', capsRel + ' — 缺失 core:default，SHIM core.invoke 会被 ACL 拒绝');
}

console.log('SUMMARY: 7 checks, ' + FAILS.length + ' FAILS');
if (FAILS.length > 0) {
  console.error('capabilities 与 Rust 窗口标签/权限契约不一致：' + FAILS.join('、'));
  process.exit(1);
}
