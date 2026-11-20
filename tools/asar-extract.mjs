#!/usr/bin/env node
/**
 * Minimal asar reader: list entries or extract them, without npm dependencies.
 *
 * Usage:
 *   node tools/asar-extract.mjs --list <app.asar> [--grep text]
 *   node tools/asar-extract.mjs --extract <app.asar> <entry> [<outFile>]
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';

const argv = process.argv.slice(2);
const flag = (name) => argv.indexOf(`--${name}`);

function readArchive(path) {
  const raw = readFileSync(path);
  const headerSize = raw.readUInt32LE(12);
  const header = JSON.parse(raw.subarray(16, 16 + headerSize).toString('utf8'));
  const base = 16 + headerSize;
  const entries = new Map();
  const walk = (node, prefix) => {
    for (const [name, child] of Object.entries(node.files ?? {})) {
      const entry = `${prefix}/${name}`;
      if (child.files) walk(child, entry);
      else if (child.offset !== undefined) entries.set(entry, { offset: Number(child.offset), size: Number(child.size) });
    }
  };
  walk(header, '');
  return { raw, base, entries };
}

const listAt = flag('list');
if (listAt >= 0) {
  const archive = readArchive(argv[listAt + 1]);
  const needle = argv[flag('grep') + 1];
  const filter = flag('grep') >= 0 ? needle : undefined;
  for (const entry of [...archive.entries.keys()].filter((name) => !filter || name.includes(filter)).sort()) {
    process.stdout.write(entry + '\n');
  }
  process.exit(0);
}

const extractAt = flag('extract');
if (extractAt >= 0) {
  const archive = readArchive(argv[extractAt + 1]);
  const entry = argv[extractAt + 2];
  const found = archive.entries.get(entry) ?? archive.entries.get(`/${entry}`);
  if (!found) {
    process.stderr.write(`not found: ${entry}\n`);
    process.exit(1);
  }
  const bytes = archive.raw.subarray(archive.base + found.offset, archive.base + found.offset + found.size);
  const out = argv[extractAt + 3];
  if (out === undefined) process.stdout.write(bytes);
  else {
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, bytes);
  }
  process.exit(0);
}

process.stderr.write('usage: --list <asar> [--grep text] | --extract <asar> <entry> [out]\n');
process.exit(2);
