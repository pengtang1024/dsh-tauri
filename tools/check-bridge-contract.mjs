#!/usr/bin/env node
/**
 * Read-only static guard for upstream desktop-bridge contract drift.
 *
 * Scans the installed runtime client packages (never writes to them) and the
 * local shell sources for hard-throw paths that would need a bridge shim.
 * Exits 0 on a known/green baseline, 1 only when a new hard-throw appears or
 * when the shell writes data-platform. Runtime absent => one SKIP line, exit 0.
 */
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const runtimeRoot = join(root, 'runtime', 'node_modules', '@deepseek-ai');
const exts = new Set(['.js', '.mjs', '.cjs', '.ts', '.tsx', '.jsx', '.map', '.html']);
const signal = /bridge|__DSH_|__ModuleLoader__|dshDesktop|dshOnboarding|boot readiness/i;
const throwCall = /(?:throw\s+new\s+Error|new\s+Error)\s*\(/g;

const FAILS = [];
const emit = (status, name, evidence = '') =>
  console.log(`${status} ${name}${evidence ? ' — ' + evidence : ''}`);
const rel = (p) => relative(root, p).split('\\').join('/');

// Offline/CI: no installed runtime client packages, nothing to guard.
if (!existsSync(runtimeRoot) || !existsSync(join(runtimeRoot, 'dsh-web-frontend')) ||
    !readdirSync(runtimeRoot).some((name) => name.startsWith('dsh-client-'))) {
  console.log(`SKIP bridge contract guard: runtime client packages not found at ${rel(runtimeRoot)} (offline/CI)`);
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

const checks = 1 + allowed.length + unexpected.length + 1;
console.log(`SUMMARY: ${checks} checks, ${FAILS.length} FAIL${FAILS.length === 1 ? '' : 'S'} (${files.length} runtime source files scanned)`);
process.exit(FAILS.length === 0 ? 0 : 1);
