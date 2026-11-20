#!/usr/bin/env node
/**
 * Host bridge: owns the upstream DSH desktop Host process and relays its Node
 * fork-IPC traffic to a line-oriented JSON protocol on stdin/stdout, so a
 * non-Node shell (the Tauri/Rust side) can drive it.
 *
 * Why this exists: `lib/host.js` reports readiness and receives native requests
 * through `process.send()` only, which requires a Node parent. Rust cannot speak
 * that channel, so this process is the Node-side half of the shell.
 *
 * Protocol (one JSON object per line, UTF-8):
 *
 *   bridge -> shell
 *     {"type":"ready","url":...,"injections":[...],"pid":...}
 *     {"type":"host","message":{...}}          any other Host event
 *     {"type":"exit","code":..,"signal":..,"stderrTail":".."}
 *     {"type":"log","stream":"stdout|stderr","chunk":".."}
 *     {"type":"pong","id":..}
 *     {"type":"error","message":".."}
 *
 *   shell -> bridge
 *     {"type":"host","message":{...}}          forwarded with process.send()
 *     {"type":"shutdown"}                      graceful Host shutdown
 *     {"type":"ping","id":..}
 *
 * Host stdout/stderr is forwarded as `log` lines: Host stdout is *not* the
 * control channel, and letting it through unmuxed would corrupt the protocol.
 *
 * Usage:
 *   node bridge/host-bridge.mjs --root <dir> --profile <dir> --runtime <dir> \
 *        [--executable <node-or-electron>] [--home <dir>] [--token <t>]
 *        [--header-limit 65536] [--inspect-port <port>]
 *
 * When --executable is omitted the current Node executable is used.
 */
import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { createServer, request as httpRequest } from 'node:http';
import { connect as netConnect } from 'node:net';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createInterface } from 'node:readline';
import { handleShellRequest, setEventSink } from './shell-api.mjs';

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const index = args.indexOf(`--${name}`);
  return index >= 0 && args[index + 1] !== undefined ? args[index + 1] : fallback;
};

const root = option('root');
const profile = option('profile');
const runtime = option('runtime');
if (!root || !profile || !runtime) {
  emit({ type: 'error', message: 'host-bridge: --root, --profile and --runtime are required' });
  process.exit(2);
}
const executable = option('executable', process.execPath);
const home = option('home', process.env.DSH_HOME);
process.env.DSH_TAURI_PROFILE ??= profile;
const token = option('token', process.env.DSH_NEXT_NATIVE_TOKEN ?? randomBytes(32).toString('base64url'));
const headerLimit = option('header-limit', '65536');
const inspectPort = option('inspect-port');
const HERE = dirname(fileURLToPath(import.meta.url));
const defaultShim = join(HERE, 'renderer-shim.js');
const shimFile = option('shim', existsSync(defaultShim) ? defaultShim : undefined);
const WITHHELD_RESPONSE_HEADERS = new Set([
  'set-cookie', 'content-encoding', 'content-length', 'transfer-encoding',
  'connection', 'keep-alive', 'te', 'trailer', 'upgrade',
  'proxy-authenticate', 'proxy-authorization'
]);
const BOOT_SCRIPT =
  '<script>globalThis.__DSH_BOOT_READY__=(function(){var deferred={};deferred.promise=new Promise(function(resolve){deferred.resolve=resolve});return deferred})();' +
  'globalThis.__DSH_FILE_UPLOAD__={fetch:(input,init)=>fetch(input,init)};<\/script>';
const SHIM_ROUTE = '/__dsh_tauri__/shim.js';
const asNode = !/[\\/]node(?:\.exe)?$/i.test(executable);

function emit(message) {
  process.stdout.write(JSON.stringify(message) + '\n');
}

setEventSink(emit);

let auth;                 // { host, cookie } once the Host is ready and authenticated
let proxyOrigin;          // loopback origin the WebView loads
let shimSource;           // renderer shim served by the proxy (optional)
if (shimFile !== undefined) shimSource = readFileSync(shimFile, 'utf8');

/** Exchange the Host launch URL for the authority-bound session cookie. */
async function authenticate(url) {
  const response = await fetch(url, {
    redirect: 'manual',
    headers: { 'x-dsh-desktop-renderer': token }
  });
  const setCookie = response.headers.get('set-cookie');
  await response.body?.cancel();
  if (response.status !== 303 || setCookie === null) {
    throw new Error(`Desktop Host authentication failed (status ${response.status})`);
  }
  const end = setCookie.indexOf(';');
  return end < 0 ? setCookie : setCookie.slice(0, end);
}

/**
 * Inject the shell boot script (and optional shim) into an HTML document served
 * by the Host, mirroring the Electron shell's `serveWebDocument` wrapper.
 */
function injectDocument(html) {
  const tags = BOOT_SCRIPT + (shimSource === undefined ? '' : `<script src="${SHIM_ROUTE}"></script>`);
  const head = html.indexOf('<head>');
  if (head < 0) return tags + html;
  return html.slice(0, head + 6) + tags + html.slice(head + 6);
}

/** Forward one HTTP request to the Host with the shell's credentials. */
function forward(request, response) {
  if (request.url === SHIM_ROUTE && shimSource !== undefined) {
    response.writeHead(200, { 'content-type': 'text/javascript; charset=utf-8', 'cache-control': 'no-store' });
    response.end(shimSource);
    return;
  }
  const upstream = new URL(request.url ?? '/', auth.host.origin);
  const headers = { ...request.headers };
  delete headers.host;
  delete headers.origin;
  delete headers.cookie;
  delete headers['sec-fetch-site'];
  delete headers['x-dsh-desktop-renderer'];
  headers.cookie = auth.cookie;
  headers.origin = auth.host.origin;
  headers['sec-fetch-site'] = 'same-origin';
  headers['x-dsh-desktop-renderer'] = token;

  const outgoing = httpRequest({
    hostname: auth.host.hostname,
    port: auth.host.port,
    path: request.url,
    method: request.method,
    headers
  }, (upstreamResponse) => {
    const outgoingHeaders = {};
    for (const [name, value] of Object.entries(upstreamResponse.headers)) {
      if (!WITHHELD_RESPONSE_HEADERS.has(name)) outgoingHeaders[name] = value;
    }
    if ((request.url ?? '').startsWith('/plugins/')) outgoingHeaders['cache-control'] = 'no-store';
    const type = String(upstreamResponse.headers['content-type'] ?? '');
    if (type.includes('text/html') && upstreamResponse.statusCode === 200) {
      const chunks = [];
      upstreamResponse.on('data', (chunk) => chunks.push(chunk));
      upstreamResponse.on('end', () => {
        const body = Buffer.from(injectDocument(Buffer.concat(chunks).toString('utf8')), 'utf8');
        outgoingHeaders['content-length'] = String(body.length);
        response.writeHead(upstreamResponse.statusCode ?? 502, outgoingHeaders);
        response.end(body);
      });
      return;
    }
    response.writeHead(upstreamResponse.statusCode ?? 502, outgoingHeaders);
    upstreamResponse.pipe(response);
  });
  outgoing.on('error', (error) => {
    if (!response.headersSent) response.writeHead(502, { 'content-type': 'text/plain' });
    response.end(String(error));
  });
  request.pipe(outgoing);
}

/** Write to a socket that may already be gone (browser navigations race upgrades). */
function writeQuietly(socket, data) {
  if (socket.destroyed || !socket.writable) return;
  try {
    socket.write(data);
  } catch {
    socket.destroy();
  }
}

/** Tunnel a WebSocket upgrade to the Host with the shell's credentials. */
function tunnel(request, socket, head) {
  const headers = { ...request.headers };
  // The Host binds the session to its own origin, so the forwarded handshake must
  // present the Host's authority — not the loopback proxy's.
  delete headers.host;
  delete headers.cookie;
  delete headers.origin;
  delete headers['sec-fetch-site'];
  delete headers['x-dsh-desktop-renderer'];
  headers.cookie = auth.cookie;
  headers['x-dsh-desktop-renderer'] = token;
  headers.origin = auth.host.origin;
  headers['sec-fetch-site'] = 'same-origin';
  const upstream = new URL(request.url ?? '/', auth.host.origin);
  const debugHeaders = Object.keys(headers).filter((name) => name !== 'cookie').map((name) => name === 'x-dsh-desktop-renderer' ? `${name}=<token>` : name).join(',');
  process.stderr.write(`[bridge] tunnel ${request.url} headers: ${debugHeaders}\n`);
  const outgoing = httpRequest({
    hostname: auth.host.hostname,
    port: auth.host.port,
    path: request.url,
    method: request.method,
    headers
  });
  outgoing.on('upgrade', (upstreamResponse, upstreamSocket, upstreamHead) => {
    process.stderr.write(`[bridge] upgrade ${request.url} -> ${upstreamResponse.statusCode}\n`);
    const lines = [`HTTP/1.1 ${upstreamResponse.statusCode} ${upstreamResponse.statusMessage}`];
    for (const [name, value] of Object.entries(upstreamResponse.headers)) {
      if (Array.isArray(value)) for (const item of value) lines.push(`${name}: ${item}`);
      else if (value !== undefined) lines.push(`${name}: ${value}`);
    }
    writeQuietly(socket, lines.join('\r\n') + '\r\n\r\n');
    if (upstreamHead.length > 0) writeQuietly(socket, upstreamHead);
    if (head.length > 0) writeQuietly(upstreamSocket, head);
    upstreamSocket.pipe(socket);
    socket.pipe(upstreamSocket);
    const close = () => {
      upstreamSocket.destroy();
      socket.destroy();
    };
    socket.on('error', close);
    upstreamSocket.on('error', close);
    socket.on('close', close);
    upstreamSocket.on('close', close);
  });
  outgoing.on('response', (upstreamResponse) => {
    process.stderr.write(`[bridge] upgrade ${request.url} -> refused ${upstreamResponse.statusCode}\n`);
    socket.write(`HTTP/1.1 ${upstreamResponse.statusCode} ${upstreamResponse.statusMessage}\r\n`);
    for (const [name, value] of Object.entries(upstreamResponse.headers)) {
      if (Array.isArray(value)) for (const item of value) socket.write(`${name}: ${item}\r\n`);
      else if (value !== undefined) socket.write(`${name}: ${value}\r\n`);
    }
    const chunks = [];
    upstreamResponse.on('data', (chunk) => chunks.push(chunk));
    upstreamResponse.on('end', () => {
      process.stderr.write(`[bridge] upgrade ${request.url} refused ${upstreamResponse.statusCode}: ${Buffer.concat(chunks).toString('utf8').slice(0, 300)}\n`);
    });
    socket.write('\r\n');
    upstreamResponse.pipe(socket);
  });
  // Do not tear the tunnel down on the ClientRequest's own 'close': after a 101
  // the request object is finished while the sockets stay live.
  outgoing.on('error', () => socket.destroy());
  socket.on('error', () => outgoing.destroy());
  outgoing.end();
}

/** Loopback reverse proxy that the WebView loads instead of the Host origin. */
function startProxy() {
  const server = createServer((request, response) => {
    if (request.url === SHIM_ROUTE && shimSource !== undefined) {
      response.writeHead(200, { 'content-type': 'text/javascript; charset=utf-8', 'cache-control': 'no-store' });
      response.end(shimSource);
      return;
    }
    handleShellRequest(request, response)
      .then((handled) => {
        if (!handled) forward(request, response);
      })
      .catch((error) => {
        if (!response.headersSent) response.writeHead(500, { 'content-type': 'text/plain' });
        response.end(String(error));
      });
  });
  server.on('upgrade', tunnel);
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      resolve({ server, origin: `http://127.0.0.1:${address.port}` });
    });
  });
}

let stderrTail = '';
const child = spawn(
  executable,
  [
    '--expose-internals',
    `--max-http-header-size=${headerLimit}`,
    ...(inspectPort === undefined ? [] : [`--inspect=127.0.0.1:${inspectPort}`]),
    `${root}/lib/host.js`,
    root,
    profile,
    runtime
  ],
  {
    cwd: profile,
    env: {
      ...process.env,
      ...(asNode ? { ELECTRON_RUN_AS_NODE: '1' } : {}),
      ...(home === undefined ? {} : { DSH_HOME: home }),
      ...(token === undefined ? {} : { DSH_NEXT_NATIVE_TOKEN: token })
    },
    stdio: ['ignore', 'pipe', 'pipe', 'ipc']
  }
);

for (const [stream, name] of [
  [child.stdout, 'stdout'],
  [child.stderr, 'stderr']
]) {
  stream.setEncoding('utf8');
  stream.on('data', (chunk) => {
    if (name === 'stderr') {
      stderrTail = (stderrTail + chunk).slice(-16384);
      process.stderr.write(chunk);
      return;
    }
    emit({ type: 'log', stream: name, chunk });
  });
}

let announced = false;
child.on('message', (message) => {
  if (message && typeof message === 'object' && message.type === 'ready') {
    if (announced) return;
    announced = true;
    const hostUrl = message.url;
    const injections = message.injections ?? [];
    authenticate(hostUrl).then(async (cookie) => {
      auth = { host: new URL(hostUrl), cookie };
      const proxy = await startProxy();
      proxyOrigin = proxy.origin;
      emit({
        type: 'ready',
        url: proxy.origin + '/',
        hostUrl,
        injections,
        pid: child.pid
      });
    }).catch((error) => emit({ type: 'error', message: `host bridge: ${error.message}` }));
    return;
  }
  if (message && typeof message === 'object' && message.type === 'shutdown-complete') {
    emit({ type: 'host', message });
    return;
  }
  emit({ type: 'host', message });
});

child.on('error', (error) => emit({ type: 'error', message: `host bridge: ${error.message}` }));

child.on('close', (code, signal) => {
  emit({ type: 'exit', code, signal, stderrTail });
  process.exit(0);
});

function send(message) {
  if (!child.connected) {
    emit({ type: 'error', message: 'host bridge: Host IPC channel is closed' });
    return;
  }
  try {
    child.send(message);
  } catch (error) {
    emit({ type: 'error', message: `host bridge: ${error.message}` });
  }
}

const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on('line', (line) => {
  const text = line.trim();
  if (text === '') return;
  let command;
  try {
    command = JSON.parse(text);
  } catch (error) {
    emit({ type: 'error', message: `host bridge: invalid command JSON: ${error.message}` });
    return;
  }
  switch (command?.type) {
    case 'host':
      send(command.message);
      break;
    case 'shutdown':
      send({ type: 'shutdown' });
      break;
    case 'ping':
      emit({ type: 'pong', id: command.id ?? null });
      break;
    default:
      emit({ type: 'error', message: `host bridge: unknown command ${String(command?.type)}` });
  }
});

input.on('close', () => send({ type: 'shutdown' }));

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => {
    send({ type: 'shutdown' });
    setTimeout(() => {
      child.kill('SIGKILL');
      process.exit(0);
    }, 5000).unref();
  });
}
