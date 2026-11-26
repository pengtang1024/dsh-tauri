#!/usr/bin/env node
/**
 * Read-only guard: the version in package.json, both places in
 * package-lock.json (top-level and packages[""]), src-tauri/tauri.conf.json
 * and src-tauri/Cargo.toml [package] must stay in sync (a release bump that
 * misses one file leaves the CLI version differing from the bundle/package
 * version). package.json is the baseline.
 *
 * Only node:fs / node:path / node:url are used; nothing is written.
 * Prints one PASS/FAIL per check with file:line and the actual value, then
 * "SUMMARY: 5 checks, M FAILS". Exits 1 when any check fails, else 0.
 */
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function readText(file) {
  try { return readFileSync(join(root, file), 'utf8'); } catch { return null; }
}

function jsonVersion(file) {
  const text = readText(file);
  if (text === null) return { value: null, line: '?' };
  let value = null;
  try { value = JSON.parse(text).version; } catch { /* reported as not found */ }
  if (typeof value !== 'string' || value.trim() === '') value = null;
  let line = '?';
  if (value !== null) {
    const lines = text.split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      const m = lines[i].match(/^\s*"version"\s*:\s*"([^"]+)"/);
      if (m && m[1] === value) { line = i + 1; break; }
    }
  }
  return { value, line };
}

function lockVersion(file, nested) {
  const text = readText(file);
  if (text === null) return { value: null, line: '?' };
  let value = null;
  try {
    const parsed = JSON.parse(text);
    value = nested ? parsed?.packages?.['']?.version : parsed.version;
  } catch { /* reported as not found */ }
  if (typeof value !== 'string' || value.trim() === '') value = null;
  let line = '?';
  if (value !== null) {
    const lines = text.split(/\r?\n/);
    let inRootPackage = false;
    for (let i = 0; i < lines.length; i++) {
      const lineText = lines[i];
      if (nested && /^\s{4}""\s*:\s*\{/.test(lineText)) { inRootPackage = true; continue; }
      if (nested && inRootPackage && /^\s{4}"[^"]+"\s*:/.test(lineText)) break;
      if (nested && !inRootPackage) continue;
      const indent = nested ? /^\s{6}"version"/ : /^\s{2}"version"/;
      if (!indent.test(lineText)) continue;
      const m = lineText.match(/"version"\s*:\s*"([^"]+)"/);
      if (m && m[1] === value) { line = i + 1; break; }
    }
  }
  return { value, line };
}

function cargoPackageVersion(file) {
  const text = readText(file);
  if (text === null) return { value: null, line: '?' };
  let inPackage = false;
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (/^\s*\[[^\]]+\]/.test(line)) {
      if (inPackage) break;
      if (/^\s*\[package\]\s*$/.test(line)) inPackage = true;
      continue;
    }
    if (!inPackage) continue;
    const m = line.match(/^\s*version\s*=\s*"([^"]+)"/);
    if (m) return { value: m[1], line: i + 1 };
  }
  return { value: null, line: '?' };
}

const baseline = jsonVersion('package.json');
const checks = [
  { name: 'package.json version', file: 'package.json', baseline: true, ...baseline },
  { name: 'package-lock.json version', file: 'package-lock.json', ...lockVersion('package-lock.json', false) },
  { name: 'package-lock.json packages[""] version', file: 'package-lock.json', ...lockVersion('package-lock.json', true) },
  { name: 'tauri.conf.json version', file: 'src-tauri/tauri.conf.json', ...jsonVersion('src-tauri/tauri.conf.json') },
  { name: 'Cargo.toml [package] version', file: 'src-tauri/Cargo.toml', ...cargoPackageVersion('src-tauri/Cargo.toml') },
];

const FAILS = [];
for (const check of checks) {
  const where = check.file + ':' + check.line;
  const actual = check.value === null ? 'VERSION NOT FOUND' : check.value;
  const ok = check.value !== null && (check.baseline || check.value === baseline.value);
  if (ok) {
    console.log('PASS ' + check.name + ' — ' + where + ' — 实际 ' + actual +
      (check.baseline ? '（基准）' : ' / 基准 ' + (baseline.value ?? 'N/A')));
  } else {
    console.log('FAIL ' + check.name + ' — ' + where + ' — 实际 ' + actual +
      (check.baseline ? '' : ' / 基准 ' + (baseline.value ?? 'N/A')));
    FAILS.push(check.name);
  }
}

console.log('SUMMARY: ' + checks.length + ' checks, ' + FAILS.length + ' FAILS');
if (FAILS.length > 0) {
  console.error('五处版本号不一致（' + FAILS.join('、') +
    '）：请同步五处版本号（package.json / package-lock.json 顶层 / package-lock.json packages[""] / src-tauri/tauri.conf.json / src-tauri/Cargo.toml [package]）。');
  process.exit(1);
}
