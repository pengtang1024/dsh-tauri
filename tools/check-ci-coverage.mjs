#!/usr/bin/env node
/**
 * Read-only guard: every leaf script run by package.json's "verify:fast"
 * must also be executed by a GitHub Actions step in .github/workflows/build.yml.
 * This prevents a local guard from being added without wiring it into CI.
 *
 * Leaf names come from /npm run (\S+)/g over scripts["verify:fast"];
 * CI names come from /run:\s*npm run (\S+)/g over build.yml.
 * Only node:fs / node:path / node:url are used; nothing is written.
 * Prints one PASS/FAIL per leaf script, then
 * "SUMMARY: N checks, M FAILS". Exits 1 when any leaf is missing, else 0.
 */
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
const workflow = readFileSync(join(root, '.github/workflows/build.yml'), 'utf8');

const fast = pkg.scripts?.['verify:fast'] ?? '';
const leaves = [...new Set([...fast.matchAll(/npm run (\S+)/g)].map((m) => m[1]))];
const ciScripts = new Set([...workflow.matchAll(/run:\s*npm run (\S+)/g)].map((m) => m[1]));

const FAILS = [];
for (const script of leaves) {
  if (ciScripts.has(script)) {
    console.log('PASS ' + script + ' — 已被 CI 执行');
  } else {
    console.log('FAIL ' + script + ' — CI 中缺少 npm run ' + script);
    FAILS.push(script);
  }
}

console.log('SUMMARY: ' + leaves.length + ' checks, ' + FAILS.length + ' FAILS');
if (FAILS.length > 0) {
  console.error('verify:fast 的叶子脚本未被 CI 覆盖：' + FAILS.join('、'));
  process.exit(1);
}
