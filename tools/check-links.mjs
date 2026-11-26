#!/usr/bin/env node
/**
 * Read-only guard: every Markdown relative link in the scanned docs must point
 * to an existing path. Scans all .md files under the repository root and docs/.
 * http(s):, mailto: and pure #anchor links are skipped; a relative link with
 * a fragment/query is checked by its path portion. The destination is decoded
 * and resolved against the directory of the document that contains it.
 * Only node:fs / node:path / node:url are used; nothing is written.
 * Prints PASS/FAIL with file:line -> target, then
 * "SUMMARY: N checks, M FAILS". Exits 1 when any link is broken, else 0.
 */
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

const docs = [...new Set([
  ...readdirSync(root).filter((name) => /\.md$/.test(name)).sort().map((name) => name),
  ...readdirSync(join(root, 'docs')).filter((name) => /\.md$/.test(name)).sort().map((name) => 'docs/' + name),
])];

const linkPattern = /\[[^\]]*\]\(([^)]*)\)/g;
let checks = 0;
const FAILS = [];

for (const doc of docs) {
  const file = join(root, doc);
  const lines = readFileSync(file, 'utf8').split(/\r?\n/);
  lines.forEach((line, index) => {
    linkPattern.lastIndex = 0;
    let match;
    while ((match = linkPattern.exec(line)) !== null) {
      let target = match[1].trim();
      if (/^(https?:|mailto:)/i.test(target) || target.startsWith('#')) continue;
      // Markdown optional title: [text](path "title")
      if (/\s/.test(target)) target = target.split(/\s/, 1)[0];
      if (target === '') continue;
      let decoded = target;
      try { decoded = decodeURIComponent(target); } catch { /* keep raw target */ }
      const pathPart = decoded.split(/[#?]/, 1)[0];
      if (pathPart === '') continue;
      checks++;
      const where = doc + ':' + (index + 1);
      if (existsSync(resolve(dirname(file), pathPart))) {
        console.log('PASS ' + where + ' -> ' + target);
      } else {
        console.log('FAIL ' + where + ' -> ' + target);
        FAILS.push(where + ' -> ' + target);
      }
    }
  });
}

console.log('SUMMARY: ' + checks + ' checks, ' + FAILS.length + ' FAILS');
if (FAILS.length > 0) {
  console.error('文档相对链接目标不存在：' + FAILS.join('、'));
  process.exit(1);
}
