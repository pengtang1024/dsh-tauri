# DSH Desktop contract (reverse-engineered)

This document records the interface between the official DeepSeek Harness Desktop
shell (Electron) and the `@deepseek-ai/*` runtime it hosts. It was recovered from
the installed application `resources/app.asar` (`lib/main.js`,
`lib/desktop-runtime-*.js`, `lib/host-process-*.js`, `lib/web-document-*.js`,
`lib/preload-*.cjs`, `lib/desktop-contract-*.js`) and verified experimentally with
[`tools/probe-host.mjs`](../tools/probe-host.mjs).

Everything here is what `dsh-tauri` re-implements; keep this file in sync when the
upstream contract changes.

## 1. Process model

```
Shell (Electron main / dsh-tauri)
  └─ spawn: <node-like> --expose-internals --max-http-header-size=65536 \
             <root>/lib/host.js <runtimeDir> <projectDir> <primaryRuntime>
       stdio: ["ignore","pipe","pipe","ipc"]        # fork IPC channel on fd 3
     Host (plain Node, no Electron imports)
       └─ starts the DSH profile, prints `ready` over fork IPC
```

| value | meaning |
| --- | --- |
| `root` | directory holding `lib/` (Electron: `resources/app.asar`) |
| `runtimeDir` | same as `root` for the desktop host |
| `projectDir` | profile directory, e.g. `~/.dsh/profiles/desktop` |
| `primaryRuntime` | `<resources>/runtime/<platform>/<arch>/primary-runtime` |
| header limit | `64 * 1024` (`MAX_HEADER_BYTES`) |

Environment:

```jsonc
{
  "ELECTRON_RUN_AS_NODE": "1",              // only when the executable is Electron
  "DSH_HOME": "~/.dsh",
  "DSH_NEXT_NATIVE_TOKEN": "<32 random bytes, base64url>",
  "DSH_NEXT_PREFERENCES": "{...DEFAULT_PREFERENCES}",
  "DSH_NEXT_TRUSTED_HOSTS": "[]",
  "DSH_NEXT_SYSTEM_PROXY": "{}",            // optional
  "DSH_TELEMETRY_DISABLED": "1"             // safe mode only
}
```

The host's `ready` event carries `{ url, injections }`, where `url` is
`http://127.0.0.1:<port>/?token=<token>` and `injections` is an array of
index-injection descriptors collected from the host web server.

## 2. Authentication

```
GET <url>                      with  x-dsh-desktop-renderer: <DSH_NEXT_NATIVE_TOKEN>
→ 303 + set-cookie
```

The cookie's first `name=value` pair is the Host session cookie. Every subsequent
request to the Host must carry **both** the cookie and the renderer header —
a request with only the cookie is rejected with `403` (verified).

`NATIVE_ACCESS_HEADER = "x-dsh-desktop-renderer"`; the token must match
`^[A-Za-z0-9_-]{43}$`.

### Access policy

`decideDesktopBrowserAccess(access, request)`:

- exact renderer token → `renderer` (allowed),
- otherwise if ordinary browser access is disabled (default) → `denied` (`403`),
- otherwise → `browser` (allowed unless the URL carries a `dsh-desktop-*` query key).

## 3. Web application

The client is the regular `@deepseek-ai/dsh-web-frontend` bundle, served by the
Host over HTTP. It is a plugin loader: `index.html` preloads
`/plugins/??<pkg>/client.js,...` bundles, then ES modules under `/assets/`.

The Electron shell serves `/`, `/index.html`, `/assets/**`, `/favicon.svg` and
`/manifest.webmanifest` from disk (injecting `APP_BOOT_SCRIPT` and a dark loading
style) and proxies everything else to the Host.

`APP_BOOT_SCRIPT`:

```html
<script>
  globalThis.__DSH_BOOT_READY__ = Promise.withResolvers();
  globalThis.__DSH_FILE_UPLOAD__ = { fetch: (input, init) => fetch(input, init) };
</script>
```

Proxied request rewriting (Electron `forwardWebRequest`):

- drop: `host`, `origin`, `cookie`, `sec-fetch-site`, `x-dsh-desktop-renderer`
- set: `cookie`, `x-dsh-desktop-renderer`, `origin: <host origin>`,
  `sec-fetch-site: same-origin`
- `redirect: manual`
- response: withhold `set-cookie`, `content-encoding`, `content-length`,
  `transfer-encoding`, `connection`, `keep-alive`, `te`, `trailer`, `upgrade`,
  `proxy-authenticate`, `proxy-authorization`; force `cache-control: no-store`
  for `/plugins/**`

The client's RPC/stream channel is a **WebSocket** (`openConnectionWebSocket`,
based on `document.baseURI`), so a shell that serves the app from a custom
protocol cannot work; it must proxy WebSocket upgrades too. HMR uses
`EventSource`.

## 4. Renderer bridge (Electron preload)

Only exposed when `location.protocol === "dsh-app:" && location.hostname === "app"`:

| global | members |
| --- | --- |
| `dshOnboarding` | `setActive(active)` |
| `dshDesktopSetup` | `read()`, `dismissAccount(profile)`, `finish(profile, selection)` |
| `desktopNext` | `renderer.profile()`, `renderer.report(status)`, `renderer.diagnostic(report)`, `permissions.*`, `onOpenSettings`, `state()`, `browserLinks()`, `command(cmd)` |
| `dshDesktop` | `protocolVersion: 1`, `browser.*`, keyboard actions, file actions, file drag |
| `dshDesktopBoot` | `ready() → { injections, streamBaseUrl, documentId, profile }`, `failed(message)` |
| `__DSH_DIRECTORY_PICKER__` | `pick()` |
| `dshPlatform` | `open(page, bounds)`, `setBounds(bounds)`, `close()` |
| `__DSH_HOST_PATHS__` | `pathFor(File)` |

The desktop client plugin (`dsh-desktop-next/client.js`) is part of the plugin
list, so it must be able to detect that these globals are absent (web mode) —
that is what makes an incremental port possible.

## 5. Host ↔ shell messages (fork IPC)

Host → shell (`isDesktopHostEvent`):

| type | payload | shell reaction |
| --- | --- | --- |
| `ready` | `url`, `injections` | authenticate, load the app |
| `platform-session` | `session` \| `null` | platform account state |
| `platform-login` | `{action:"open",url}` \| `{action:"close",focus}` | open/close login window |
| `fatal` | `message`, `diagnostic` | error dialog, restart |
| `notification` | `notification` | native notification |
| `permission` | `requestId`, `action`, `permission` | OS permission prompt |
| `desktop-action` | `restart` \| `terminal` | restart / open terminal window |
| `update-tasks` | `requestId`, `active`, `error?` | tray/menu progress |
| `logging-config` | `requestId`, `error?` | apply log level |
| `browser-access` | `requestId`, `error?` | toggle LAN exposure |
| `injections` | `requestId`, `injections?` | boot payload |
| `startup-entries` | `requestId`, `entries?` | startup diagnostics |
| `memory` | usage | metrics |
| `shutdown-complete` | — | exit |

Shell → host: `{type:"permission-result",requestId,snapshot|error}`,
`{type:"shutdown"}`, and `{type:<injections|update-tasks|startup-entries|browser-access|logging-config>,requestId,...}`.

## 6. Porting notes

- `lib/host.js` has no Electron imports: the host runs under plain Node too.
  The only Electron dependency is the ASAR archive itself, so a product build
  ships loose `@deepseek-ai/*` packages plus a bundled Node.
- Because the renderer needs WebSocket upgrades, `dsh-tauri` serves the app from
  an ephemeral loopback HTTP proxy (which adds the renderer header to HTTP
  requests and WebSocket handshakes) instead of a custom protocol.
- `browserAccess` is a user preference; the shell must not flip it silently —
  the proxy exists precisely so it does not have to.

## Platform 登录（`platform-login`）

Host 在账号需要登录时通过 fork IPC 发 `{type:'platform-login', action:'open', url}`
（Host 侧来源：`reference-upstream/lib/host.js:570` 的 `watchPlatformLogin`）。`action:'close'`
表示登录窗该关掉，带 `focus:boolean`。

官方 Electron 用独立分区窗口（`main.js:3777` `PLATFORM_LOGIN_PARTITION='dsh-platform-login'`，
无 `persist:` 前缀，避免 Platform cookie 落盘），并在 `webRequest` 里拦截导航
（`classifyPlatformLoginRequest`）：只放行 https 与 about/data/blob 子资源，Host 同源的
`/oauth/callback` 必须由外壳以「原生渲染器」（`Cookie` + `x-dsh-desktop-renderer`）重放
（`forwardPlatformLoginCallback`，`CALLBACK_TIMEOUT=15min`）。

本项目的做法：Tauri 开一个 `login` 窗口加载该 URL，`on_navigation` 检测到
`/oauth/callback` 且 origin 等于 Host origin 时取消导航，把 URL 交给 bridge
（stdin `{type:'platform-callback'}`），bridge 经回环代理重放同一路径 —— 代理本来就补
cookie 与渲染器令牌，等价于官方的重放；拿到状态码后回到 Rust，关掉登录窗。
