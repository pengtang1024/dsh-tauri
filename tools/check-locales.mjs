#!/usr/bin/env node
/**
 * Read-only guard: the seven shipped locale/*.json files must be valid JSON,
 * carry non-empty meta.title / meta.description / meta.detail strings, and
 * point meta.detail at an existing document. The on-disk language set must be
 * exactly zh/en/es/fr/de/ru/pt.
 *
 * Only node:fs / node:path / node:url are used; nothing is written.
 * Prints one PASS/FAIL per check with file:line (or path), then
 * "SUMMARY: N checks, M FAILS". Exits 1 when any check fails, else 0.
 */
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const localeDir = join(root, 'locale');
const expectedLanguages = ['zh', 'en', 'es', 'fr', 'de', 'ru', 'pt'];
const metaFields = ['title', 'description', 'detail'];

const checks = [];
function record(name, where, ok, note) {
  checks.push({ name, where, ok, note });
  console.log((ok ? 'PASS ' : 'FAIL ') + name + ' — ' + where + (note ? ' — ' + note : ''));
}

// Locate the line of a top-level meta field for file:line diagnostics.
function fieldLine(lines, field) {
  const pattern = new RegExp('^\\s*"' + field + '"\\s*:');
  const index = lines.findIndex((line) => pattern.test(line));
  return index < 0 ? '?' : String(index + 1);
}

let files = [];
let listingOk = true;
try {
  files = readdirSync(localeDir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith('.json'))
    .map((entry) => entry.name)
    .sort();
} catch (error) {
  listingOk = false;
  record('扫描 locale/*.json', 'locale/', false, String(error.message || error));
}

if (listingOk) {
  const foundLanguages = files.map((name) => name.slice(0, -'.json'.length)).sort();
  const wanted = [...expectedLanguages].sort();
  const sameSet = foundLanguages.length === wanted.length &&
    foundLanguages.every((lang, index) => lang === wanted[index]);
  record('语言集合恰好 7 种', 'locale/*.json', sameSet,
    '期望 ' + wanted.join(',') + ' / 实际 ' + (foundLanguages.join(',') || '(空)'));

  for (const name of files) {
    const relativePath = 'locale/' + name;
    const absolutePath = join(localeDir, name);
    let text;
    try {
      text = readFileSync(absolutePath, 'utf8');
    } catch (error) {
      record(relativePath + ' 可读', relativePath, false, String(error.message || error));
      continue;
    }

    let data;
    try {
      data = JSON.parse(text);
      record(relativePath + ' JSON 合法', relativePath, true);
    } catch (error) {
      record(relativePath + ' JSON 合法', relativePath, false, String(error.message || error));
      continue;
    }

    const lines = text.split(/\r?\n/);
    const meta = data && typeof data === 'object' && !Array.isArray(data) ? data.meta : null;
    const values = {};
    for (const field of metaFields) {
      const value = meta && typeof meta === 'object' && !Array.isArray(meta) ? meta[field] : undefined;
      const ok = typeof value === 'string' && value.trim().length > 0;
      values[field] = value;
      record(relativePath + ' meta.' + field + ' 非空字符串',
        relativePath + ':' + fieldLine(lines, field), ok,
        ok ? '' : '缺失或非非空字符串');
    }

    if (typeof values.detail === 'string' && values.detail.trim().length > 0) {
      const detailPath = join(root, values.detail);
      record(relativePath + ' meta.detail 文档存在',
        relativePath + ':' + fieldLine(lines, 'detail') + ' → ' + values.detail,
        existsSync(detailPath), existsSync(detailPath) ? '' : '文档不存在');
    }
  }
}

const fails = checks.filter((check) => !check.ok);
console.log('SUMMARY: ' + checks.length + ' checks, ' + fails.length + ' FAILS');
if (fails.length > 0) {
  console.error('本地化资源一致性检查失败：' + fails.map((check) => check.name).join('、'));
  process.exitCode = 1;
}
