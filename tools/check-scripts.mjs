#!/usr/bin/env node
/**
 * Read-only guard: syntax-check every .mjs under tools/ except this file.
 * Discovery uses node:fs only; each script is checked with
 * spawnSync(process.execPath, ['--check', file], { stdio: 'inherit' }).
 * Prints "PASS N tool scripts syntax" and exits 0 when all pass;
 * prints "FAIL <filename>" for each failure and exits 1.
 */
import { spawnSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const self = fileURLToPath(import.meta.url);
const toolsDir = dirname(self);

function* toolScripts(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) { yield* toolScripts(path); continue; }
    if (entry.isFile() && entry.name.endsWith('.mjs') && resolve(path) !== resolve(self)) yield path;
  }
}

const files = [...toolScripts(toolsDir)].sort();
const failed = [];
for (const file of files) {
  const result = spawnSync(process.execPath, ['--check', file], { stdio: 'inherit' });
  if (result.status !== 0) failed.push(relative(toolsDir, file));
}

if (failed.length > 0) {
  for (const name of failed) console.error(`FAIL ${name}`);
  process.exitCode = 1;
} else {
  console.log(`PASS ${files.length} tool scripts syntax`);
}
