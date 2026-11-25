#!/usr/bin/env node
/** Run the real runtime probe with an isolated, self-cleaning DSH_HOME. */
import { spawnSync } from 'node:child_process';
import { mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const tmp = join(root, `.dsh-probe-${process.pid}`);
mkdirSync(tmp, { recursive: true });
try {
  const result = spawnSync(process.execPath, ['tools/probe-web.mjs'], {
    cwd: root,
    stdio: 'inherit',
    env: { ...process.env, DSH_HOME: tmp }
  });
  process.exitCode = result.status ?? 1;
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
