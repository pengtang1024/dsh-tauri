#!/usr/bin/env node
/**
 * Read-only guard: capabilities/default.json must stay in sync with the labels
 * the Rust shell actually builds, keep the core:default permission that lets the
 * SHIM call core.invoke, and — since Tauri 2.12 — grant every application command
 * the web content invokes. Without the last part a window-label or permission
 * drift silently revokes every native capability at runtime with
 * "Command <name> not allowed by ACL".
 *
 * Why the origin matters: the main window loads the DSH UI with
 * WebviewUrl::External (http://127.0.0.1:<port>), and tauri 2.12.7 rejects an
 * application command from a non-local origin unless a capability matches that
 * origin (capability.remote.urls) *and* an application permission allows it
 * (tauri-2.12.7/src/webview/mod.rs:2080).
 *
 * Checks:
 * 1. capabilities/default.json exists and is valid JSON.
 * 2. windows is a non-empty array of non-empty strings.
 * 3. the windows set equals exactly the set of labels passed as the second
 *    argument of WebviewWindowBuilder::new(app, "<label>", ...) in
 *    src-tauri/src 下所有 .rs（递归；Rust 里的重复标签按集合收敛）。
 * 4. permissions is a non-empty array containing "core:default".
 * 5. every application command invoked from the web content (native.rs::SHIM
 *    call('<cmd>') and dist/boot.html core.invoke('<cmd>')) is allowed by a
 *    permission listed in the capability.
 * 6. every application permission the capability references exists in a
 *    src-tauri/permissions 下的 .toml（拼错同样会让 Rust 构建失败，只是
 *    错误来自 tauri-build；这里更早报出来）。
 * 7. when a window is created with WebviewUrl::External, the capability declares
 *    remote.urls (otherwise Tauri treats that page as remote and denies every
 *    application command).
 * 8. those remote.urls cover the shell's default loopback host.
 * 9. splash/local origins stay allowed (capability.local must not be false while
 *    a local-origin window is declared).
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
const permsDir = join(root, 'src-tauri', 'permissions');
const bootPage = join(root, 'src-tauri', 'dist', 'boot.html');

function* sourceFiles(dir) {
  let entries;
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) { yield* sourceFiles(path); continue; }
    if (entry.isFile() && entry.name.endsWith('.rs')) yield path;
  }
}

function* permissionFiles(dir) {
  let entries;
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) { yield* permissionFiles(path); continue; }
    if (entry.isFile() && entry.name.endsWith('.toml')) yield path;
  }
}

const FAILS = [];
let CHECKS = 0;
function pass(name, detail) { CHECKS++; console.log('PASS ' + name + ' — ' + detail); }
function fail(name, detail) { CHECKS++; console.log('FAIL ' + name + ' — ' + detail); FAILS.push(name); }
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
const rustSources = new Map();
let externalWindow = null;
const externalPattern = /\bWebviewUrl\s*::\s*External\b/g;
for (const file of sourceFiles(srcDir)) {
  const text = readFileSync(file, 'utf8');
  rustSources.set(file, text);
  for (const m of text.matchAll(builderPattern)) {
    if (!rustLabels.has(m[2])) rustLabels.set(m[2], relative(root, file) + ':' + lineOf(text, m.index));
    // A builder that passes WebviewUrl::External in its arguments makes this
    // capability's origin remote: record where, for check 7.
    const tail = text.slice(m.index, m.index + 600);
    const ext = externalPattern.exec(tail);
    externalPattern.lastIndex = 0;
    if (ext && externalWindow === null) {
      externalWindow = relative(root, file) + ':' + lineOf(text, m.index + ext.index);
    }
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
const permissionEntries = caps && Array.isArray(caps.permissions) ? caps.permissions : null;
const permissionIds = permissionEntries
  ? permissionEntries.map((p) => (typeof p === 'string' ? p : p && p.identifier)).filter((p) => typeof p === 'string' && p.length > 0)
  : [];
if (permissionEntries && permissionEntries.length > 0 && permissionEntries.every((p) => (typeof p === 'string' && p.length > 0) || (p && typeof p.identifier === 'string'))) {
  pass('permissions 字段非空字符串数组', capsRel + ' — ' + formatSet(permissionIds));
} else {
  fail('permissions 字段非空字符串数组', capsRel + ' — permissions 缺失/为空/含非法值');
}
if (permissionIds.includes('core:default')) {
  pass('permissions 含 core:default', capsRel + ' — SHIM 的 core.invoke 权限在位');
} else {
  fail('permissions 含 core:default', capsRel + ' — 缺失 core:default，SHIM core.invoke 会被 ACL 拒绝');
}

// --- 5. every command invoked from web content is allowed ---
// Where each invoked command comes from, for the failure message.
const invokedFrom = new Map();
function recordInvoked(command, where) {
  if (!invokedFrom.has(command)) invokedFrom.set(command, where);
}
const shimPattern = /call\s*\(\s*'([a-z][a-z0-9_]*)'/g;
const nativePath = join(srcDir, 'native.rs');
const nativeText = rustSources.get(nativePath) ?? '';
const shimStart = nativeText.indexOf('pub const SHIM: &str = r#"');
const shimEnd = shimStart < 0 ? -1 : nativeText.indexOf('"#;', shimStart);
if (shimStart >= 0 && shimEnd > shimStart) {
  const shimBody = nativeText.slice(shimStart, shimEnd);
  for (const m of shimBody.matchAll(shimPattern)) {
    if (!invokedFrom.has(m[1])) {
      recordInvoked(m[1], relative(root, nativePath) + ':' + lineOf(nativeText, shimStart + m.index));
    }
  }
} else {
  fail('定位 SHIM 注入脚本', relative(root, nativePath) + ' 未找到 pub const SHIM: &str = r#" … "#;（无法核对 SHIM 调用的命令）');
}
const bootInvokePattern = /core\.invoke\s*\(\s*'([a-z][a-z0-9_]*)'/g;
try {
  const bootText = readFileSync(bootPage, 'utf8');
  for (const m of bootText.matchAll(bootInvokePattern)) {
    recordInvoked(m[1], relative(root, bootPage) + ':' + lineOf(bootText, m.index));
  }
} catch {
  fail('定位启动页', relative(root, bootPage) + ' 不存在或无法读取（无法核对启动页调用的命令）');
}

// Parse the application permission manifest: identifier -> commands.allow.
const permissionCommands = new Map();
const permissionIdsDeclared = new Map();
for (const file of permissionFiles(permsDir)) {
  const text = readFileSync(file, 'utf8');
  const rel = relative(root, file);
  const lines = text.split(/\r?\n/);
  let current = null;
  let collecting = false;
  let buffer = [];
  let allowAt = 0;
  for (const [index, line] of lines.entries()) {
    if (!collecting) {
      const idMatch = line.match(/^\s*identifier\s*=\s*"([^"]+)"/);
      if (idMatch) {
        current = idMatch[1];
        if (!permissionIdsDeclared.has(current)) permissionIdsDeclared.set(current, rel + ':' + (index + 1));
      }
      const allowMatch = line.match(/^\s*commands\.allow\s*=\s*\[(.*)$/);
      if (allowMatch && current) {
        allowAt = index + 1;
        buffer = [allowMatch[1]];
        collecting = !allowMatch[1].includes(']');
        if (collecting) continue;
        for (const quoted of buffer.join(' ').matchAll(/"([^"]+)"/g)) {
          const list = permissionCommands.get(current) ?? [];
          list.push(quoted[1]);
          permissionCommands.set(current, list);
        }
        continue;
      }
    } else {
      buffer.push(line);
      if (line.includes(']')) {
        collecting = false;
        for (const quoted of buffer.join(' ').matchAll(/"([^"]+)"/g)) {
          const list = permissionCommands.get(current) ?? [];
          list.push(quoted[1]);
          permissionCommands.set(current, list);
        }
        buffer = [];
      }
    }
  }
  void allowAt;
}

const appPermissions = permissionIds.filter((id) => !id.includes(':'));
const allowedCommands = new Set();
for (const id of appPermissions) for (const command of permissionCommands.get(id) ?? []) allowedCommands.add(command);

const invoked = [...invokedFrom.keys()].sort((a, b) => a.localeCompare(b));
if (invoked.length === 0) {
  fail('提取 SHIM/启动页调用的应用命令', '未提取到任何 call(\'<cmd>\') 或 core.invoke(\'<cmd>\')，守卫无从核对');
} else if (appPermissions.length === 0) {
  fail('应用命令被 capabilities 放行', capsRel + ' 未引用任何应用权限（无 ":" 前缀的标识），SHIM 命令会被 ACL 拒绝：' + formatSet(invoked));
} else {
  const missing = invoked.filter((command) => !allowedCommands.has(command));
  if (missing.length === 0) {
    pass('应用命令被 capabilities 放行', invoked.map((c) => c).join('、') + '（' + invokedFrom.size + ' 个，来源 ' + [...new Set([...invokedFrom.values()].map((where) => where.replace(/:\d+$/, '')))].join('、') + '）');
  } else {
    fail('应用命令被 capabilities 放行',
      '未被放行: ' + missing.map((c) => c + '(' + invokedFrom.get(c) + ')').join('、') +
      '；已放行: ' + formatSet([...allowedCommands].sort()));
  }
}

// --- 6. capability references only declared application permissions ---
if (appPermissions.length === 0) {
  fail('capability 引用的应用权限存在', capsRel + ' 未引用任何应用权限');
} else {
  const unknown = appPermissions.filter((id) => !permissionIdsDeclared.has(id));
  if (unknown.length === 0) {
    pass('capability 引用的应用权限存在', formatSet(appPermissions.map((id) => id + '(' + permissionIdsDeclared.get(id) + ')')));
  } else {
    fail('capability 引用的应用权限存在',
      'src-tauri/permissions/**\/*.toml 中不存在: ' + formatSet(unknown) + '；已声明: ' + formatSet([...permissionIdsDeclared.keys()]));
  }
}

// --- 7. an External window needs remote.urls ---
const remoteUrls = caps && caps.remote && Array.isArray(caps.remote.urls) ? caps.remote.urls : [];
if (externalWindow === null) {
  pass('External 窗口需要 remote.urls', '未发现 WebviewUrl::External 窗口，无需远程来源');
} else if (remoteUrls.length > 0) {
  pass('External 窗口需要 remote.urls', externalWindow + ' → remote.urls = ' + formatSet(remoteUrls));
} else {
  fail('External 窗口需要 remote.urls',
    externalWindow + ' 用 WebviewUrl::External 加载页面（远程来源），但 ' + capsRel +
    ' 没有 remote.urls：Tauri 2.12 会以 "Command <name> not allowed by ACL" 拒绝该页面的每个应用命令');
}

// --- 8. remote.urls cover the default loopback host ---
let defaultHost = '127.0.0.1';
const dshText = rustSources.get(join(srcDir, 'dsh.rs')) ?? '';
const hostMatch = dshText.match(/DSH_TAURI_HOST"\s*\)\s*\.unwrap_or_else\(\s*\|_\|\s*"([^"]+)"/);
if (hostMatch) defaultHost = hostMatch[1];
const hostCovered = remoteUrls.some((pattern) => pattern.startsWith('http://' + defaultHost + ':') || pattern === 'http://' + defaultHost);
// tauri-utils parses each pattern with the urlpattern tokenizer inside
// generate_context!, which panics on bracketed hosts ("invalid name; must be at
// least length 1"), so an IPv6 literal here breaks the whole crate at compile
// time. Loopback IPv4/hostname patterns are the only safe form.
const unsafePattern = remoteUrls.find((pattern) => /[\[\]]/.test(pattern));
if (unsafePattern) {
  fail('remote.urls 可被 urlpattern 解析', capsRel + ' — ' + unsafePattern + ' 带方括号（IPv6 字面量），generate_context! 会在编译期 panic；只写 http://<主机>:* 形式');
} else if (remoteUrls.length > 0) {
  pass('remote.urls 可被 urlpattern 解析', formatSet(remoteUrls) + ' 均为 http://<主机>:* 形式');
}
if (remoteUrls.length === 0) {
  fail('remote.urls 覆盖默认回环主机', 'remote.urls 为空，无法覆盖 ' + defaultHost);
} else if (hostCovered) {
  pass('remote.urls 覆盖默认回环主机', '默认主机 ' + defaultHost + ' 被 ' + formatSet(remoteUrls) + ' 覆盖');
} else {
  fail('remote.urls 覆盖默认回环主机',
    '默认主机 ' + defaultHost + '（dsh.rs 的 DSH_TAURI_HOST 默认值）没有对应的 remote.urls 条目，端口随进程变化，需写成 http://' + defaultHost + ':*');
}

// --- 9. local origins stay allowed while a local-origin window exists ---
if (caps && caps.local === false) {
  fail('local 来源保持放行', capsRel + ' 设了 local:false，splash 等本地来源窗口会被拒绝');
} else if (windows && windows.includes('splash')) {
  pass('local 来源保持放行', 'splash 走 tauri://localhost 本地来源，capability.local 未关闭');
} else {
  pass('local 来源保持放行', 'capability.local 未关闭');
}

console.log('SUMMARY: ' + CHECKS + ' checks, ' + FAILS.length + ' FAILS');
if (FAILS.length > 0) {
  console.error('capabilities 与 Rust 窗口标签/权限契约不一致：' + FAILS.join('、'));
  process.exit(1);
}
