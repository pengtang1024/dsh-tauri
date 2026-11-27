#!/usr/bin/env node
/**
 * Read-only static guard for upstream desktop-bridge contract drift.
 *
 * Scans the installed runtime client packages (never writes to them) and the
 * local shell sources for hard-throw paths that would need a bridge shim.
 * Also checks that the local shell still injects the bridge SHIM into both
 * windows (boot + main), and that tauri.conf.json keeps withGlobalTauri true
 * so globalThis.__TAURI__.core.invoke exists for the SHIM and boot page, and
 * that the shell never injects a dshDesktop global (documented decision:
 * docs/DESKTOP_BRIDGE.zh.md §5).
 * Exits 0 on a known/green baseline, 1 only when a new hard-throw appears,
 * when the shell writes data-platform, when a SHIM injection point is
 * missing, when withGlobalTauri is not true, or when the shell injects a
 * dshDesktop global.
 *
 * Runtime absent (offline/CI before install): the contract checks cannot run.
 * Default exit stays 0 so offline base gates remain usable, but the output is
 * explicitly marked "STATUS: SKIPPED" and the SUMMARY line carries
 * "(SKIPPED: ...; N contract checks NOT executed)" — never a bare "0 FAILS"
 * that could be mistaken for a pass. Pass --strict (or set
 * DSH_TAURI_REQUIRE_BRIDGE=1) to promote that SKIP to FAIL/exit 1 in
 * environments that expect the runtime to be installed.
 * GUARD_CHECK_COUNT must match the number of emit() calls, enforced by the
 * end-of-file self-check.
 */
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Number of checks reported in SUMMARY. When adding/removing a check below,
// update this constant and the count in CHANGELOG.md ("契约漂移守卫") together.
export const GUARD_CHECK_COUNT = 13;

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const runtimeRoot = join(root, 'runtime', 'node_modules', '@deepseek-ai');
const exts = new Set(['.js', '.mjs', '.cjs', '.ts', '.tsx', '.jsx', '.map', '.html']);
const signal = /bridge|__DSH_|__ModuleLoader__|dshDesktop|dshOnboarding|boot readiness/i;
const throwCall = /(?:throw\s+new\s+Error|new\s+Error)\s*\(/g;

const FAILS = [];
let emittedChecks = 0;
const emit = (status, name, evidence = '') => {
  emittedChecks++;
  console.log(`${status} ${name}${evidence ? ' — ' + evidence : ''}`);
};
const rel = (p) => relative(root, p).split('\\').join('/');

// Offline/CI: no installed runtime client packages, nothing to guard.
// Default: loud SKIP (exit 0) so the other offline gates still run.
// Strict: the same condition is a FAIL, for setups that installed the runtime.
const strict = process.argv.includes('--strict') ||
  /^(?:1|true|yes|on)$/i.test(process.env.DSH_TAURI_REQUIRE_BRIDGE ?? '');
if (!existsSync(runtimeRoot) || !existsSync(join(runtimeRoot, 'dsh-web-frontend')) ||
    !readdirSync(runtimeRoot).some((name) => name.startsWith('dsh-client-'))) {
  const missing = `runtime client packages not found at ${rel(runtimeRoot)}`;
  const installHint = 'npm install --prefix runtime @deepseek-ai/dsh';
  if (strict) {
    console.error(`FAIL bridge contract guard: ${missing} (--strict / DSH_TAURI_REQUIRE_BRIDGE=1)`);
    console.error(`  ${GUARD_CHECK_COUNT} contract checks were NOT executed; install with: ${installHint}`);
    console.error(`SUMMARY: 0 checks, 1 FAILS (SKIPPED: ${missing}; promoted to FAIL by strict mode)`);
    process.exit(1);
  }
  console.log(`SKIP bridge contract guard: ${missing} (offline/CI)`);
  console.log(`  ${GUARD_CHECK_COUNT} contract checks were NOT executed — this is NOT a PASS.`);
  console.log(`  To execute them: ${installHint}`);
  console.log(`SUMMARY: 0 checks, 0 FAILS (SKIPPED: ${missing}; ${GUARD_CHECK_COUNT} contract checks NOT executed)`);
  console.log('STATUS: SKIPPED (bridge contract NOT verified)');
  process.exit(0);
}

// Eight hard throws already accounted for by the shipped SHIM. Template
// expressions collapse to <expr> so a known message stays known across builds.
const allowlist = new Set([
  'dsh-client-modules | client-modules: window.__DSH_BOOT__ is missing or not an object',
  'dsh-client-modules | client-modules: window.__ModuleLoader__.create called after module-system boot',
  'dsh-client-modules | client-modules: bundle <expr> loaded without registering <expr> via __ModuleLoader__.load',
  'dsh-client-shortcuts | Desktop keyboard bridge unavailable',
  'dsh-client-shortcuts | Desktop shortcuts bridge unavailable',
  'dsh-client-ui-settings-account | desktop login bridge unavailable',
  'dsh-web-frontend | desktop web: boot readiness is missing',
  'dsh-web-frontend | web boot: window.__ModuleLoader__ bootstrap facade is missing',
]);

function normalize(message) {
  const collapsed = message
    .replace(/\s+/g, ' ')
    .replace(/(?:\$\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\})+/g, '<expr>')
    .replace(/(?:['"`])\s*\+\s*[^+;]+\+\s*['"`]/g, '<expr>')
    .replace(/\bhttps?:\/\/\S+/g, '<expr>');
  // Quotes around a template placeholder carry no contract meaning: bundle
  // ${url} loaded ... and bundle "${url}" loaded ... are the same throw.
  return collapsed.replace(/['"`](<expr>)['"`]/g, '$1').trim();
}

// Read the first string/template literal argument of an Error constructor.
// Do not evaluate code: stop at the matching closing delimiter.
function firstArgument(text, openParen) {
  let i = openParen + 1;
  while (i < text.length && /\s/.test(text[i])) i++;
  const quote = text[i];
  if (quote !== "'" && quote !== '"' && quote !== '`') return undefined;
  let out = '';
  let escaped = false;
  for (i++; i < text.length; i++) {
    const c = text[i];
    if (escaped) { out += c; escaped = false; continue; }
    if (c === '\\') { escaped = true; continue; }
    if (c === quote) return out;
    out += c;
  }
  return undefined;
}

function* walk(dir) {
  let entries;
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    if (entry.name === 'node_modules') continue;
    const path = join(dir, entry.name);
    if (entry.isDirectory()) { yield* walk(path); continue; }
    if (entry.isFile() && exts.has(entry.name.slice(entry.name.lastIndexOf('.')))) {
      yield path;
    }
  }
}

// Source-map sourcesContent embeds the original .ts source; scan it too, and
// attribute hits to the original file:line instead of the minified bundle.
function* unitsOf(path) {
  const raw = readFileSync(path, 'utf8');
  yield { path, lineOffset: 0, text: raw };
  if (!path.endsWith('.map')) return;
  try {
    const map = JSON.parse(raw);
    const sources = Array.isArray(map.sources) ? map.sources : [];
    const contents = Array.isArray(map.sourcesContent) ? map.sourcesContent : [];
    for (let i = 0; i < contents.length; i++) {
      if (typeof contents[i] !== 'string') continue;
      yield { path: resolve(dirname(path), sources[i] ?? `source-${i}`), lineOffset: 0, text: contents[i] };
    }
  } catch { /* malformed map: the raw line above is still scanned */ }
}

// Virtual source-map paths (../../packages/...) point into the upstream
// monorepo, not the installed package. Attribute them to the owning bundle.
const packageOf = (unit, sourceFile) => {
  const match = sourceFile.match(/@deepseek-ai\/([^/]+)/);
  if (match) return match[1];
  if (unit.path === sourceFile) {
    const self = unit.path.match(/@deepseek-ai\/([^/]+)/);
    return self ? self[1] : 'unknown';
  }
  const owner = rel(unit.path).match(/@deepseek-ai\/([^/]+)/);
  return owner ? owner[1] : 'unknown';
};

// ---------------------------------------------------------------------------
// 1) The directory-picker global must still be consumed upstream.
// ---------------------------------------------------------------------------
const pickerPkg = join(runtimeRoot, 'dsh-client-ui-directory-picker-native');
const pickerCandidates = [
  join(pickerPkg, 'src', 'client', 'index.ts'),
  join(pickerPkg, 'lib', 'client.js'),
];
const pickerHits = pickerCandidates
  .filter((p) => existsSync(p) && readFileSync(p, 'utf8').includes('__DSH_DIRECTORY_PICKER__'))
  .map(rel);
if (pickerHits.length > 0) emit('PASS', 'directory-picker global __DSH_DIRECTORY_PICKER__ still consumed', pickerHits[0]);
else emit('WARN', 'directory-picker global __DSH_DIRECTORY_PICKER__ not found (manual review)', pickerCandidates.filter(existsSync).map(rel).join(', ') || pickerPkg);

// ---------------------------------------------------------------------------
// 2) Baseline hard throws: known entries PASS, anything new FAILs.
// ---------------------------------------------------------------------------
const frontendDirs = [join(runtimeRoot, 'dsh-web-frontend', 'dist'), join(runtimeRoot, 'dsh-web-frontend', 'src')];
const clientDirs = readdirSync(runtimeRoot, { withFileTypes: true })
  .filter((e) => e.isDirectory() && e.name.startsWith('dsh-client-'))
  .flatMap((e) => [join(runtimeRoot, e.name, 'src'), join(runtimeRoot, e.name, 'lib')]);
const scanDirs = [...frontendDirs, ...clientDirs].filter(existsSync);
const files = [];
for (const dir of scanDirs) for (const file of walk(dir)) files.push(file);

const seen = new Map();
for (const file of files) {
  let text;
  try { text = readFileSync(file, 'utf8'); } catch { continue; }
  if (!signal.test(text)) continue;
  for (const unit of unitsOf(file)) {
    throwCall.lastIndex = 0;
    let match;
    while ((match = throwCall.exec(unit.text)) !== null) {
      const args = unit.text.slice(match.index, match.index + 400);
      const message = firstArgument(args, args.indexOf('('));
      if (message === undefined || !signal.test(message)) continue;
      const normalized = normalize(message);
      const key = `${packageOf(unit, file)} | ${normalized}`;
      if (seen.has(key)) continue;
      const line = unit.text.slice(0, match.index).split('\n').length + unit.lineOffset;
      const evidence = `${rel(unit.path)}:${line} [${rel(file)}]`;
      seen.set(key, { key, allowed: allowlist.has(key), evidence: preferSource(seen.get(key)?.evidence, evidence, file), raw: message });
    }
  }
}

// A source-map .ts location is more useful evidence than a minified bundle
// line; keep the original when both forms were found.
function preferSource(existing, candidate, sourceFile) {
  if (!existing) return candidate;
  const virtual = sourceFile.endsWith('.map') || /\/dist\/assets\//.test(sourceFile);
  if (virtual && !/\.map|\/dist\/assets\//.test(existing)) return existing;
  return candidate;
}

const allowed = [...seen.values()].filter((h) => h.allowed).sort((a, b) => a.key.localeCompare(b.key));
const unexpected = [...seen.values()].filter((h) => !h.allowed).sort((a, b) => a.key.localeCompare(b.key));
for (const hit of allowed) emit('PASS', `known hard-throw baseline: ${hit.key}`, hit.evidence);
for (const hit of unexpected) {
  emit('FAIL', `new hard-throw needs bridge review: ${hit.key}`, hit.evidence);
  FAILS.push(`${hit.key} @ ${hit.evidence}`);
}
if (allowed.length === 0 && unexpected.length === 0) {
  emit('WARN', 'no signal-bearing hard throws found (baseline drift)', scanDirs.map(rel).slice(0, 3).join(', '));
}

// ---------------------------------------------------------------------------
// 3) The shell must never write data-platform (shortcut hard-throw risk).
// ---------------------------------------------------------------------------
const localRoots = [join(root, 'src-tauri', 'src'), join(root, 'src-tauri', 'dist')].filter(existsSync);
let platformHits = 0;
for (const dir of localRoots) {
  for (const file of walk(dir)) {
    let text;
    try { text = readFileSync(file, 'utf8'); } catch { continue; }
    text.split('\n').forEach((line, i) => {
      if (!/data-platform/.test(line)) return;
      if (!/(?:setAttribute\s*\(|dataset\.|\.dataset\[|data-platform\s*=)/.test(line)) return;
      platformHits++;
      emit('FAIL', 'data-platform write in shell source (shortcut hard-throw risk)', `${rel(file)}:${i + 1}`);
      FAILS.push(`data-platform write @ ${rel(file)}:${i + 1}`);
    });
  }
}
if (platformHits === 0) emit('PASS', 'no data-platform write in src-tauri/src or src-tauri/dist', localRoots.map(rel).join(', '));

// ---------------------------------------------------------------------------
// 4) The local shell must still inject the bridge SHIM into both windows.
// ---------------------------------------------------------------------------
const shellUiPath = join(root, 'src-tauri', 'src', 'shell_ui.rs');
const shimInjection = /\.initialization_script\s*\(\s*crate::native::SHIM\s*\)/g;
const shimLines = [];
if (existsSync(shellUiPath)) {
  const shellUi = readFileSync(shellUiPath, 'utf8');
  let match;
  while ((match = shimInjection.exec(shellUi)) !== null) {
    shimLines.push(shellUi.slice(0, match.index).split('\n').length);
  }
}
const shimEvidence = shimLines.map((line) => `${rel(shellUiPath)}:${line}`).join(', ');
if (shimLines.length === 2) {
  emit('PASS', 'bridge SHIM injected into both windows', shimEvidence);
} else if (shimLines.length < 2) {
  emit('FAIL', 'SHIM not injected into both windows', `found ${shimLines.length}${shimEvidence ? ' at ' + shimEvidence : ''}`);
  FAILS.push(`SHIM not injected into both windows: found ${shimLines.length}${shimEvidence ? ' at ' + shimEvidence : ''}`);
} else {
  emit('WARN', 'SHIM injected into more than two windows (manual review)', `${shimLines.length} sites: ${shimEvidence}`);
}

// ---------------------------------------------------------------------------
// 5) withGlobalTauri must be true so __TAURI__.core.invoke is available to the
//    injected SHIM and the boot page.
// ---------------------------------------------------------------------------
const tauriConfPath = join(root, 'src-tauri', 'tauri.conf.json');
const globalTauriFail = 'withGlobalTauri must be true: SHIM and boot page rely on __TAURI__.core.invoke';
let globalTauriOk = false;
let globalTauriEvidence = rel(tauriConfPath);
try {
  const confText = readFileSync(tauriConfPath, 'utf8');
  const conf = JSON.parse(confText);
  if (conf && conf.app && conf.app.withGlobalTauri === true) {
    const line = confText.split('\n').findIndex((l) => /"withGlobalTauri"\s*:/.test(l)) + 1;
    globalTauriOk = true;
    if (line > 0) globalTauriEvidence += ':' + line;
  }
} catch { /* missing/unparseable config => FAIL below */ }
if (globalTauriOk) {
  emit('PASS', 'withGlobalTauri is true (global Tauri bridge available)', globalTauriEvidence);
} else {
  emit('FAIL', globalTauriFail, globalTauriEvidence);
  FAILS.push(globalTauriFail + ' @ ' + globalTauriEvidence);
}

// ---------------------------------------------------------------------------
// 6) The shell must never define/inject a dshDesktop global (documented §5).
//    Only local src-tauri sources are scanned; the signal regex above that
//    mentions dshDesktop guards upstream hard-throws and is not a shell write.
// ---------------------------------------------------------------------------
const desktopRoots = [
  { dir: join(root, 'src-tauri', 'src'), suffixes: new Set(['.rs']) },
  { dir: join(root, 'src-tauri', 'dist'), suffixes: new Set(['.html', '.js']) },
].filter((entry) => existsSync(entry.dir));
const dshDesktopInject = /\b(?:globalThis|window|self|global)\s*\.\s*dshDesktop\s*=|(?:^\s*|[{[,\s]*)['"`]?dshDesktop['"`]?\s*:|\binitialization_script\b[^\n]*dshDesktop|\bdshDesktop\b[^\n]*\binitialization_script\b/;
function* walkDesktop(dir, suffixes) {
  let entries;
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    if (entry.name === 'node_modules') continue;
    const path = join(dir, entry.name);
    if (entry.isDirectory()) { yield* walkDesktop(path, suffixes); continue; }
    if (entry.isFile() && suffixes.has(entry.name.slice(entry.name.lastIndexOf('.')))) yield path;
  }
}
let desktopHits = 0;
for (const { dir, suffixes } of desktopRoots) {
  for (const file of walkDesktop(dir, suffixes)) {
    let text;
    try { text = readFileSync(file, 'utf8'); } catch { continue; }
    text.split('\n').forEach((line, i) => {
      if (!dshDesktopInject.test(line)) return;
      desktopHits++;
      const evidence = `${rel(file)}:${i + 1}`;
      emit('FAIL', 'dshDesktop global injection in shell source (see docs/DESKTOP_BRIDGE.zh.md §5)', evidence);
      FAILS.push(`dshDesktop global injection @ ${evidence} — see docs/DESKTOP_BRIDGE.zh.md §5: providing dshDesktop flips desktop sign-in/keyboard behavior and must stay unimplemented`);
    });
  }
}
if (desktopHits === 0) emit('PASS', 'dshDesktop global not injected (documented decision)', desktopRoots.map((entry) => rel(entry.dir)).join(', '));

const checks = GUARD_CHECK_COUNT;
console.log(`SUMMARY: ${checks} checks, ${FAILS.length} FAIL${FAILS.length === 1 ? '' : 'S'} (${files.length} runtime source files scanned)`);
if (emittedChecks !== GUARD_CHECK_COUNT) {
  console.error(`GUARD_CHECK_COUNT mismatch: constant=${GUARD_CHECK_COUNT}, actual emit calls=${emittedChecks}`);
  process.exit(1);
}
process.exit(FAILS.length === 0 ? 0 : 1);
