#!/usr/bin/env node
/**
 * Read-only guard: every #[tauri::command] function under src-tauri/src (recursive)
 * must be registered in main.rs's generate_handler! block. A missing entry
 * compiles and passes every test, but the page gets "command not found" at
 * runtime — so the SHIM capability silently stops working.
 *
 * Only node:fs / node:path / node:url are used; nothing is written.
 * Prints one PASS/FAIL per defined command with file:line, then
 * "SUMMARY: N checks, M FAILS". Exits 1 when any command is unregistered.
 */
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const srcDir = join(root, 'src-tauri', 'src');
const mainPath = join(srcDir, 'main.rs');

function* sourceFiles(dir) {
  let entries;
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) { yield* sourceFiles(path); continue; }
    if (entry.isFile() && entry.name.endsWith('.rs')) yield path;
  }
}

// An attribute may be followed by other attributes and blank lines before
// the function; only the next `pub fn` counts as the command definition.
const commandAttr = /^\s*#\[tauri::command\]\s*$/;
const pubFn = /^\s*pub\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/;
const defined = [];
for (const file of sourceFiles(srcDir)) {
  const text = readFileSync(file, 'utf8');
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    if (!commandAttr.test(lines[i])) continue;
    let j = i + 1;
    while (j < lines.length && (lines[j].trim() === '' || /^\s*#\[/.test(lines[j]))) j++;
    if (j >= lines.length) continue;
    const m = lines[j].match(pubFn);
    if (!m) continue;
    defined.push({ name: m[1], where: relative(root, file) + ':' + (j + 1) });
  }
}
defined.sort((a, b) => a.name.localeCompare(b.name));

const mainText = readFileSync(mainPath, 'utf8');
const handler = mainText.match(/generate_handler!\s*\[([\s\S]*?)\]/);
const registered = new Set();
if (handler) {
  for (const m of handler[1].matchAll(/[A-Za-z_][A-Za-z0-9_]*::([A-Za-z_][A-Za-z0-9_]*)\s*,?/g)) {
    registered.add(m[1]);
  }
}

const FAILS = [];
for (const { name, where } of defined) {
  if (registered.has(name)) {
    console.log('PASS ' + name + ' — ' + where + ' — 已注册');
  } else {
    console.log('FAIL ' + name + ' — ' + where + ' — 未在 main.rs 的 generate_handler! 注册');
    FAILS.push(name);
  }
}

console.log('SUMMARY: ' + defined.length + ' checks, ' + FAILS.length + ' FAILS');
if (FAILS.length > 0) {
  console.error('存在未注册的 Tauri 命令：' + FAILS.join('、'));
  process.exit(1);
}
