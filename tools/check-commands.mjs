#!/usr/bin/env node
/**
 * Read-only guards for Tauri command names:
 * 1. every #[tauri::command] under src-tauri/src (recursive) is registered in
 *    main.rs's generate_handler! block;
 * 2. every command name invoked from the JS side is actually registered, and
 *    every registered command is invoked somewhere on the JS side.
 *
 * A missing registration compiles and passes every other test, but the page
 * gets "command not found" at runtime — so a renamed command silently stops
 * the SHIM / boot page. Only node:fs / node:path / node:url are used; nothing
 * is written. Prints one PASS/FAIL per check with file:line, then
 * "SUMMARY: N checks, M FAILS". Exits 1 when any check fails.
 */
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const srcDir = join(root, 'src-tauri', 'src');
const mainPath = join(srcDir, 'main.rs');
const nativePath = join(srcDir, 'native.rs');
const bootPath = join(root, 'src-tauri', 'dist', 'boot.html');

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
// Commands may be `async` (native dialogs, idle waits): they are still commands
// and must be registered like any other.
const pubFn = /^\s*pub\s+(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/;
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
function fail(name) { FAILS.push(name); }
function location(text, index) {
  return text.slice(0, index).split(/\r?\n/).length;
}

// --- JS side: call('<name>') inside the SHIM raw string and core.invoke('<name>') in boot.html ---
const nativeText = readFileSync(nativePath, 'utf8');
const shimMatch = nativeText.match(/pub\s+const\s+SHIM\s*:\s*&str\s*=\s*r#"([\s\S]*?)"#\s*;/);
const shimText = shimMatch ? shimMatch[1] : '';
const shimWhere = relative(root, nativePath);
const shimCalls = new Map();
if (shimMatch) {
  // Names are dynamic at the call site (call(command, args)); extract the
  // literal arguments passed to call(...) in the injected script.
  for (const m of shimText.matchAll(/\bcall\(\s*(['"])([A-Za-z_][A-Za-z0-9_]*)\1\s*(?:,|\))/g)) {
    if (!shimCalls.has(m[2])) shimCalls.set(m[2], shimWhere + ':' + location(shimText, m.index));
  }
} else {
  fail('SHIM raw string not found in ' + shimWhere);
}

const bootText = readFileSync(bootPath, 'utf8');
const bootRel = relative(root, bootPath);
const bootCalls = new Map();
for (const m of bootText.matchAll(/\binvoke\(\s*(['"])([A-Za-z_][A-Za-z0-9_]*)\1\s*(?:,|\))/g)) {
  if (!bootCalls.has(m[2])) bootCalls.set(m[2], bootRel + ':' + location(bootText, m.index));
}

const jsCalls = new Map([...shimCalls, ...bootCalls]);
const jsNames = [...jsCalls.keys()].sort((a, b) => a.localeCompare(b));
const registeredNames = [...registered].sort((a, b) => a.localeCompare(b));

// 1. Existing Rust-defined ⊆ registered check (unchanged semantics).
for (const { name, where } of defined) {
  if (registered.has(name)) {
    console.log('PASS ' + name + ' — ' + where + ' — 已注册');
  } else {
    console.log('FAIL ' + name + ' — ' + where + ' — 未在 main.rs 的 generate_handler! 注册');
    fail(name);
  }
}

// 2. Every JS-invoked command must exist in the generated Tauri handler.
for (const name of jsNames) {
  const where = jsCalls.get(name);
  if (registered.has(name)) {
    console.log('PASS JS→Rust ' + name + ' — ' + where + ' — 已注册');
  } else {
    console.log('FAIL JS→Rust ' + name + ' — ' + where + ' — 未在 main.rs 的 generate_handler! 注册');
    fail('JS→Rust ' + name);
  }
}

// 3. Every registered command must be invoked from the JS side (no dead command).
for (const name of registeredNames) {
  if (jsCalls.has(name)) {
    console.log('PASS Rust→JS ' + name + ' — generate_handler! — JS 侧有调用');
  } else {
    console.log('FAIL Rust→JS ' + name + ' — generate_handler! — JS 侧无调用');
    fail('Rust→JS ' + name);
  }
}

// 4. Self-check: non-empty sources and no duplicate names inside one source.
const checks = defined.length + jsNames.length + registeredNames.length + 4;
const selfChecks = [
  [shimCalls.size > 0, 'SHIM 提取到 ' + shimCalls.size + ' 个调用'],
  [bootCalls.size > 0, 'boot 页提取到 ' + bootCalls.size + ' 个调用'],
  [registered.size > 0, 'generate_handler! 提取到 ' + registered.size + ' 个注册'],
  [jsNames.length === new Set(jsNames).size, 'JS 调用名无重复'],
];
for (const [ok, message] of selfChecks) {
  console.log((ok ? 'PASS ' : 'FAIL ') + message);
  if (!ok) fail(message);
}

console.log('SUMMARY: ' + checks + ' checks, ' + FAILS.length + ' FAILS');
if (FAILS.length > 0) {
  console.error('Tauri 命令名契约不一致：' + FAILS.join('、'));
  process.exit(1);
}
