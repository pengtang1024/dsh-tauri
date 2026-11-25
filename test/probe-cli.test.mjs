import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const probe = join(projectRoot, 'tools', 'probe-web.mjs');

/** Run the probe in a child process with a hard guard so a hang cannot wedge the suite. */
function runProbe(args, env = {}, guardMs = 4000) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [probe, ...args], {
      cwd: projectRoot,
      env: { ...process.env, ...env },
      stdio: ['ignore', 'pipe', 'pipe']
    });
    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    const guard = setTimeout(() => {
      child.kill('SIGKILL');
      reject(new Error(`probe did not exit within ${guardMs}ms; stderr: ${stderr}`));
    }, guardMs);
    child.on('error', (error) => {
      clearTimeout(guard);
      reject(error);
    });
    child.on('exit', (code, signal) => {
      clearTimeout(guard);
      resolve({ code, signal, stdout, stderr });
    });
  });
}

test('probe exits 2 when the runtime is missing', async () => {
  const missing = join(tmpdir(), `dsh-probe-missing-${process.pid}-${Date.now()}`);
  const result = await runProbe(['--runtime', missing], { DSH_PROBE_TIMEOUT_MS: '600' });
  assert.equal(result.code, 2, `stderr: ${result.stderr}`);
  assert.match(result.stderr, /missing runtime/);
});

test('probe times out when the runtime never prints its URL', async (t) => {
  const runtime = mkdtempSync(join(tmpdir(), 'dsh-probe-'));
  t.after(() => rmSync(runtime, { recursive: true, force: true }));
  const entry = join(runtime, 'node_modules', '@deepseek-ai', 'dsh', 'lib');
  mkdirSync(entry, { recursive: true });
  // A fake runtime that stays alive but never prints its URL.
  writeFileSync(join(entry, 'bin.js'), 'setTimeout(() => {}, 60000);\n');

  const result = await runProbe(['--runtime', runtime], { DSH_PROBE_TIMEOUT_MS: '600' });
  assert.equal(result.code, 1, `stderr: ${result.stderr}`);
  assert.match(result.stderr, /FAIL/);
  assert.match(result.stderr, /timeout/i);
});

test('probe echoes runtime stderr tail when the runtime exits before printing its URL', async (t) => {
  const runtime = mkdtempSync(join(tmpdir(), 'dsh-probe-'));
  t.after(() => rmSync(runtime, { recursive: true, force: true }));
  const entry = join(runtime, 'node_modules', '@deepseek-ai', 'dsh', 'lib');
  mkdirSync(entry, { recursive: true });
  // A fake runtime that emits a unique marker on stderr, then dies before its URL.
  writeFileSync(join(entry, 'bin.js'), "process.stderr.write('FAKE_STDERR_MARKER_42\\n'); process.exit(1);\n");

  const result = await runProbe(['--runtime', runtime], { DSH_PROBE_TIMEOUT_MS: '600' });
  assert.equal(result.code, 1, `stderr: ${result.stderr}`);
  assert.match(result.stderr, /FAKE_STDERR_MARKER_42/);
});
