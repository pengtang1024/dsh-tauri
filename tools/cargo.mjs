#!/usr/bin/env node
/**
 * Thin cargo wrapper so every npm script gets a writable target directory.
 *
 * The global ~/.cargo/config.toml on this machine points target-dir outside
 * the workspace (read-only for the sandbox), which makes plain `cargo run`
 * fail with "Read-only file system". Here CARGO_TARGET_DIR defaults to
 * <root>/src-tauri/target unless the caller already set it, and cargo runs
 * from <root>/src-tauri regardless of the caller's cwd.
 *
 * Usage: node tools/cargo.mjs <cargo args...>
 *   e.g. node tools/cargo.mjs run
 *        node tools/cargo.mjs test -- --nocapture
 */
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
const cargoTargetDir = process.env.CARGO_TARGET_DIR ?? join(root, 'src-tauri', 'target');

const result = spawnSync('cargo', args, {
  stdio: 'inherit',
  cwd: join(root, 'src-tauri'),
  env: { ...process.env, CARGO_TARGET_DIR: cargoTargetDir }
});

if (result.error) {
  console.error(`failed to run cargo: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
