# dsh-tauri — Detailed Guide

> This document is a faithful compilation based on `README.md` and `docs/DEVELOPMENT.md` in this repository, for a quick understanding of the project's positioning, architecture, dependencies and installation, configuration, usage, verification commands, and known limitations/boundaries. Code blocks, identifiers, numbers, paths, and commands are kept verbatim.

## 1. Project Positioning

A desktop client for [DeepSeek Harness](https://github.com/deepseek-ai) built with **Tauri 2**: the window, tray, menu, and native capabilities come from Rust + the system WebView, while Harness itself runs the official npm package `@deepseek-ai/dsh`, i.e. the pure Node service behind `dsh web`.

**Neither Electron nor DSH Desktop needs to be installed.** One `node` plus one copy of `node_modules/@deepseek-ai/dsh` is the entire runtime.

### Core Goal

**Goal:** Linux comes first with deep support — across distributions and versions — with the aim of the best compatibility, performance, and lowest resource use; it is the only platform fully adapted and verified. Windows and macOS have basic desktop support — building and running is enough, with no in-depth adaptation or optimization. Windows / macOS users are advised to download the official DeepSeek Harness desktop build directly from the [official GitHub repository](https://github.com/deepseek-ai/deepseek-harness) or other official channels; this project's Tauri shell is not the preferred option on Win/Mac. Mobile (iOS / Android) is not supported: the architecture depends on a local Node service plus the system WebView, which does not fit mobile sandbox and background restrictions.

> **Current state (as-is):** Linux is the only platform fully adapted and verified (the code currently relies on Linux-native commands `zenity` / `wl-clipboard` / `notify-send` and has only been verified on Linux + WebKitGTK 2.52; broad multi-distribution compatibility remains a goal, not a fully verified fact). Windows and macOS have basic support — they build and run, but without in-depth adaptation or optimization and without full verification. Mobile (iOS / Android) is explicitly not supported, because the architecture depends on a local Node service plus the system WebView, which does not fit mobile sandbox and background restrictions.

### How this differs from "stuffing an Electron app into Tauri"

Tauri only provides the system WebView and a Rust backend; it does not ship a Node runtime. So the key is not shelling it differently, but **which core you run**:

| Approach | Runtime | Result |
| --- | --- | --- |
| Tauri wrapping the Electron install directory | app.asar from `/opt/DSH NEXT` (the core depends on the Electron main process) | You still have to reverse-engineer the official install, and it breaks whenever upstream upgrades |
| **This project** | `@deepseek-ai/dsh` from npm, `dsh web` as a plain Node process | The core is a publicly distributed package; upgrading only needs a version-number change |

All that remains for the shell to do is three things: start the service, point the window at it, and supply the capabilities a browser cannot.

## 2. Architecture

```
┌────────────────────── Tauri 进程（Rust） ──────────────────────┐
│ 托盘 / 菜单 / 窗口状态 / 单实例 / 崩溃重启                      │
│ 原生命令：目录选择、剪贴板、通知、外链、窗口标题                │
│ window ──► http://127.0.0.1:<port>/?token=…                    │
└──────────┬───────────────────────────────────────▲─────────────┘
           │ spawn（stdout 里解析 URL）             │ invoke
┌──────────▼───────────────────────────────────────┴─────────────┐
│ node runtime/node_modules/@deepseek-ai/dsh/lib/bin.js           │
│   --profile web --host 127.0.0.1 --port 0 --no-open             │
│ 官方 Web UI + 插件 + 会话存储（DSH_HOME，默认 ~/.dsh）           │
└─────────────────────────────────────────────────────────────────┘
```

The injection script (`src-tauri/src/native.rs::SHIM`) runs before the page scripts, adding `__DSH_DIRECTORY_PICKER__` and `__DSH_TAURI__` (clipboard/notification/external links), and syncing `document.title` to the native title bar — WebKit does not do this on its own.

### Layers

| Layer | Location | Responsibility |
| --- | --- | --- |
| Service supervision | `src-tauri/src/supervisor.rs` | spawn `dsh web`, parse URL from stdout, crash restart, SIGTERM exit |
| Native capabilities | `src-tauri/src/native.rs` | directory picker, clipboard, notification, external links, window title + injection script |
| Shell | `src-tauri/src/shell_ui.rs` | window, menu, tray, window state, error page, geometry clipping |
| Single instance | `src-tauri/src/single_instance.rs` | `$XDG_RUNTIME_DIR/dsh-tauri.sock` |

### Directory layout

```
src-tauri/src/main.rs      Application entry: Tauri builder, command registration, startup flow and shutdown events
src-tauri/src/dsh.rs        运行时配置解析与信息上报
src-tauri/src/runtime.rs    运行时定位（Env → checkout → 安装目录）+ 两个开关
src-tauri/src/install.rs    npm 安装到暂存目录、带超时与失败信息
src-tauri/src/update.rs     启动后后台 `npm view` 比对版本、自动装新、notify-send
src-tauri/src/swap.rs       `.new-<pid>` 暂存 + 原子 rename，启动时清理 `.old-*`
src-tauri/src/progress.rs   启动阶段与已用时（splash 轮询）
src-tauri/src/exec.rs       带超时的子进程执行（整组进程一起收）
src-tauri/src/version.rs    语义化版本比较
src-tauri/src/supervisor.rs 监督线程：定位 → 安装 → 启动
src-tauri/src/banner.rs     从服务日志里解析 URL
src-tauri/src/native.rs     原生命令 + 注入脚本
src-tauri/src/shell_ui.rs   窗口、菜单、托盘、splash、错误页
src-tauri/src/single_instance.rs  单实例（Unix socket）
runtime/                    仅开发用：本地安装的 @deepseek-ai/dsh
tools/probe-web.mjs         运行时契约探针
docs/DEVELOPMENT.md         开发与验证手册
```

### Capability mapping

| Capability | Implementation |
| --- | --- |
| Sessions / workspaces / plugins | Official Web UI, loaded from `dsh web` |
| Directory picker, clipboard, notification, external links | Injection script + Tauri commands |
| Window title, size, tray, single instance | `shell_ui.rs` / `single_instance.rs` |
| Automatic crash restart | `supervisor.rs`, error page after at most 3 attempts |
| Runtime install/update | Automatic (installed on first launch, update checked after launch) |
| Startup progress | splash polls `runtime_status`: stage + elapsed time |

## 3. Dependencies and Installation

- Node 24+ (on PATH, or specified by `DSH_TAURI_NODE`)
- Rust 1.9x, `webkit2gtk-4.1`, `libsoup-3.0`, `librsvg2`, `libayatana-appindicator3`, `patchelf`
- Runtime helpers: `zenity` (directory picker / About panel), `wl-clipboard` (clipboard), `libnotify-bin` (`notify-send` notifications), `xdg-utils` (`xdg-open` external links)
- The Debian / Ubuntu `.deb` package declares all four helpers in `bundle.linux.deb.depends`, so they are pulled in at install time and work out of the box; AppImage and source runs require installing them manually:

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf build-essential \
  zenity wl-clipboard libnotify-bin xdg-utils
```

## 4. Build and Usage

```bash
npm install --prefix runtime @deepseek-ai/dsh   # 仅开发用：让 probe / 本仓库内启动拿到本体
npm test                                        # 运行时契约探针

cd src-tauri && cargo run
```

`cargo run` starts the service itself and opens it in a window.

Packaging (deb / AppImage) uses `npx tauri build` and does not distribute `node_modules` with the package: **the first launch installs automatically**, putting `@deepseek-ai/dsh` into `~/.dsh/dsh-tauri/runtime`. During this, splash shows the stage (looking up / installing / starting) and the elapsed time; you do not need to run any command first.

The shell looks in order along " `DSH_TAURI_RUNTIME` → in-repo `runtime/` → `~/.dsh/dsh-tauri/runtime` ". Only when **offline** do you need to install once manually:

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh
```

### Automatic updates

Once the service is up, the shell starts a background thread running `npm view @deepseek-ai/dsh version`. If the upstream version is newer than the local one, it installs automatically into the same runtime directory, then uses `notify-send` to show "已更新到 <版本>，重启生效".

- **Does not interrupt the current session**: the running service is not killed; the old directory is renamed to `<runtime>.old-<pid>` and kept for it to read from, while the new version is loaded only on the **next launch**
- **Fails silently and retries**: a failed check or install writes only one log line (`[runtime] 更新检查失败（下次启动再试）`), shows no popup, does not affect usage, and is retried on the next launch
- **Only updates directories it installed itself**: the directory specified by `DSH_TAURI_RUNTIME` and the in-repo `runtime/` belong to the user or git; the shell only logs and never writes to them

Manual upgrading still works, equivalent to having the shell skip it:

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh@latest
node tools/probe-web.mjs --runtime ~/.dsh/dsh-tauri/runtime   # 升级后先验契约
```

### Runtime lookup order

`DSH_TAURI_RUNTIME` → in-repo `runtime/` (only counts if `node_modules` exists) → `~/.dsh/dsh-tauri/runtime` → `runtime/` under the packaged resource directory.

When none is found, an error page is shown with the install command in its body.

`runtime::Source` records which tier was hit. Only `Home` and `Resource` are the shell's own directories and may be written by automatic updates; `Explicit` (`DSH_TAURI_RUNTIME`) and `Checkout` (in-repo) only get a log line and are never touched — they may contain git-tracked files.

### First-launch automatic installation

When `runtime::ensure` finds no runtime in any of the four tiers and it is not in offline mode, it calls `install::install(dir, "latest")`, installing to `~/.dsh/dsh-tauri/runtime`:

```
runtime::ensure          定位失败 → 非离线 → install_fresh
  install::npm_available 先跑 `npm --version`，缺 npm 给可读错误
  swap::Staging::new     建 <runtime>.new-<pid> 暂存目录（先清掉旧的临时树）
  exec::run              npm install --prefix <staging> --no-audit --no-fund
                          --loglevel=error @deepseek-ai/dsh@latest（600 秒超时）
  has_package            校验 node_modules/@deepseek-ai/dsh/lib/bin.js 存在
  Staging::commit        rename 换上去，旧的改名 .old-<pid>
```

- During installation, `progress::set(Stage::Installing, …)` is set, and splash polls `runtime_status` to show the stage and elapsed time;
- `exec.rs` runs npm in its own session, reaping the whole process group on timeout or exit, leaving no `postinstall` grandchild processes;
- Any failure returns a readable error plus the manual command, going through `progress::fail` → error page.

### Automatic update implementation

After the service starts, `update::spawn_once(dir, source)` starts a `dsh-update` thread (runs only once per process):

1. `version_at(dir)` reads the version from local `node_modules/@deepseek-ai/dsh/package.json`;
2. `npm view @deepseek-ai/dsh version` fetches the upstream version (90-second timeout, honoring the user's npm mirror configuration);
3. `version::is_newer` compares (pure Rust semantic comparison, including prerelease rules; see the `version.rs` unit tests);
4. If an update is available, `progress::set(Stage::Updating, …)` → `install::install(dir, "latest")` goes through the same staging + atomic replacement flow → `notify-send -a DSH "DSH 已更新" "已更新到 x.y.z，重启生效"`.

Constraints: do not kill the running service (the new version loads only on the next launch), on failure only print `[runtime] 更新检查失败（下次启动再试）` and retry on the next launch, and when `Source::updatable()` is false, only write one log line.

### Atomic replacement and cleanup

- The staging directory `<runtime>.new-<pid>` and the old tree `<runtime>.old-<pid>` are both **sibling directories** of the runtime, keeping `rename` on the same filesystem and atomic;
- `commit` first renames the old directory to `.old-<pid>`, then renames staging into place; if the second step fails, the old directory is renamed back (the error message states "旧运行时已恢复");
- `Staging::drop` deletes the staging tree when there was no commit — an interrupted install leaves no half-finished product;
- The old tree is **kept for the still-running service** (its modules were loaded from the old tree), so after an update two copies exist on disk at once (about 1.2G); `swap::clean_stale_runtime()` is called in `main.rs`'s `.setup`, after the single-instance lock is won, cleaning all `<name>.new-*` / `<name>.old-*` siblings, logging only on failure.

## 5. Configuration and Environment Variables

`cargo run` starts the service itself and opens it in a window. Common overrides:

| Variable | Purpose |
| --- | --- |
| `DSH_TAURI_RUNTIME` | runtime directory (containing `node_modules`) |
| `DSH_TAURI_NODE` | node executable |
| `DSH_TAURI_HOST` | host the service binds to, loopback only (default `127.0.0.1`; `localhost` / `::1` allowed) |
| `DSH_TAURI_NPM` | path to the npm executable (npm on PATH by default; used by automatic install/update and version detection) |
| `DSH_TAURI_PROFILE` | which profile to launch (default `web`) |
| `DSH_HOME` | data directory (default `~/.dsh`) |
| `DSH_TAURI_URL` | for development: open an already-running loopback service directly (http(s) + 127.0.0.1/localhost/[::1] only), without spawning one |

### Automatic update switches (environment variables)

| Variable | Purpose |
| --- | --- |
| `DSH_TAURI_AUTO_UPDATE=0` | disable the version check; first-launch automatic installation still runs |
| `DSH_TAURI_OFFLINE=1` | disable all network activity (install + check); can still start with an existing runtime while offline |

### The two switches (values and effects)

| Variable | Values | Effect |
| --- | --- | --- |
| `DSH_TAURI_OFFLINE` | `1/true/yes/on` | disable installation and checking (all network activity); error page if no runtime |
| `DSH_TAURI_AUTO_UPDATE` | `0/false/no/off` | disable only the version check; first-launch automatic installation still runs |

Implemented in `runtime.rs::offline()` / `auto_update_enabled()`; `auto_update_enabled` short-circuits `OFFLINE` first, so in offline mode both are off.

## 6. Key Implementation Notes

1. **The service URL comes only from stdout**: `dsh web` prints `dsh web: http://127.0.0.1:<port>/?token=…`; do not guess the port yourself — `--port 0` lets the system assign one.
2. **The window must be created directly on the URL after obtaining it**. Creating a splash window first and then `navigate`-ing makes WebKit drop the authentication redirect's `Set-Cookie` (symptom: the page shows only `dsh web authentication required`). So use a splash window with a separate label, and when the URL is hit, create a new `main` window and then destroy the splash.
3. **The injection script must run before the page scripts**: `WebviewWindowBuilder::initialization_script`. It relies on `window.__TAURI__.core.invoke` provided by `withGlobalTauri` (see `tauri.conf.json`).
4. **The token in the query string is a one-time authentication entry point**: `GET` returns 303 and issues a `dsh-auth-<authority>` cookie (`HttpOnly; SameSite=Strict`). The server generates the cookie name by authority, so it changes when the port changes.
5. **Exit goes through SIGTERM**: `dsh::shutdown()` first `kill -TERM`, and only SIGKILLs if it has not exited after 5 seconds, to avoid leaving WAL residue in the session database.
6. **Tray exit** must call `dsh::shutdown()` before `app.exit(0)`, otherwise orphan node processes are left behind.

## 7. Verification Commands

```bash
node --check tools/probe-web.mjs
npm test                                     # 拉起 dsh web，验证 303 + cookie + 客户端 HTML
cd src-tauri && cargo test && cargo build    # Rust 单测 + 构建
```

`npm test` requires the runtime to be installed first:

```bash
npm install --prefix runtime @deepseek-ai/dsh
```

### Debugging automatic installation and updates

**Fake npm** (no network, no 1.2G download; verifies the whole install/update chain): `install::npm()` recognizes `DSH_TAURI_NPM`; write an executable script that impersonates npm.

```bash
#!/usr/bin/env bash
# /tmp/fake-npm ：DSH_TAURI_NPM=/tmp/fake-npm
set -e
case "$1" in
  --version) echo "10.0.0" ;;
  view)      echo "${FAKE_UPSTREAM:-9.9.9}" ;;          # 假装上游版本
  install)                                              # npm install --prefix X …
    prefix=""; prev=""
    for a in "$@"; do [ "$prev" = "--prefix" ] && prefix="$a"; prev="$a"; done
    pkg="$prefix/node_modules/@deepseek-ai/dsh"
    mkdir -p "$pkg/lib"
    echo '{"name":"@deepseek-ai/dsh","version":"'"${FAKE_LOCAL:-1.0.0}"'"}' > "$pkg/package.json"
    : > "$pkg/lib/bin.js"
    ;;
esac
```

- **First-launch installation**: move `~/.dsh/dsh-tauri/runtime` away (`mv` to `/tmp`), then `DSH_TAURI_NPM=/tmp/fake-npm cargo run`; splash should enter the "正在安装" stage and, when done, `~/.dsh/dsh-tauri/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js` should exist; the fake package has no real service, so the window eventually stops at a startup failure — that is enough to verify the install chain.
- **Update path**: after installation, set `FAKE_LOCAL=1.0.0` and `FAKE_UPSTREAM=2.0.0` and launch again; logs should show `发现新版 2.0.0（本地 1.0.0）`, `已原子替换`, `旧运行时保留至下次启动清理`, and a sibling `<runtime>.old-<pid>` appears; launching once more should print `清理上次运行留下的 …`.
- **Switches**: `DSH_TAURI_OFFLINE=1` with no runtime → error page body carries the `npm install --prefix` command; `DSH_TAURI_AUTO_UPDATE=0` → log shows `自动更新已关闭`, but the first install still runs.
- **Install failure**: make the fake npm `install` branch `exit 1` → error page carries npm output and the manual command, and the original runtime is unaffected (`<runtime>.new-<pid>` was cleared by `Drop`).
- **Cleanup**: `pkill -x dsh-tauri`; confirm there are no `.new-*` / `.old-*` leftovers under `~/.dsh/dsh-tauri/`, and remember to delete/restore the fake npm and the moved-away runtime.

### Smoke check (manual)

```bash
cd src-tauri && cargo run          # 应看到 splash，随后出现完整 UI
curl -s -o /dev/null -w '%{http_code}\n' "$(grep -ao 'http://127.0.0.1:[0-9]*/?token=[A-Za-z0-9_-]*' /tmp/xxx.log | head -1)"
```

The window should show: session list, workspaces, model selector, and running session streams.

## 8. Verified (Linux + WebKitGTK 2.52)

- `dsh web` is spawned by the shell and `http://127.0.0.1:<port>/?token=…` is parsed from stdout; the first screen appears within 3–30 seconds (depending on cache);
- Authentication chain: token URL → 303 + `dsh-auth-*` cookie → `GET /` returns the client HTML (`tools/probe-web.mjs`);
- The window contains the complete Web UI: session list, workspaces, model selection, plugin pages, and ongoing session streams are all present;
- The window renames itself with `document.title`; closing follows the preference of "close to tray" or "quit" (the preference is the `closeToTray` field in `~/.dsh/desktop-preferences.json`: `true` = close to tray, `false` = quit, default `true`); size is restored on restart;
- A second launch focuses the existing window and exits, without starting a second service;
- The service restarts automatically after being killed (at most 3 times); if it still fails, a readable error page is shown;
- On first launch with no runtime, `npm install` runs automatically into `~/.dsh/dsh-tauri/runtime`, and splash shows the current stage and elapsed time;
- The service starts automatically after installation; when a newer upstream version exists, it is installed in the background and `notify-send` shows "重启生效", without affecting the running service;
- With `DSH_TAURI_OFFLINE=1` and no local runtime, an error page with the manual command is shown instead of a blank window;
- Updates go through a `<runtime>.new-<pid>` staging directory + atomic replacement, so an interrupted install does not damage the already-installed runtime.

## 9. Known Limitations / Boundaries

| Item | Current state |
| --- | --- |
| Platform page embedded view (usage / recharge) | Opened in a browser; no native embedding implemented |
| Old runtime takes up disk after update | The old tree is renamed `<runtime>.old-<pid>` and kept for the still-running service, **cleaned only on the next launch**; during that time it occupies about one extra copy of the runtime (about 1.2G) |
| SIGKILL residue | When the shell is `SIGKILL`ed it has no time to reap child processes, so npm's `postinstall` grandchildren may briefly remain; normal exit (SIGTERM) is unaffected |
| Windows / macOS / mobile (iOS / Android) | **Windows / macOS: basic support** — builds and runs, with no in-depth adaptation or optimization and no full verification; Windows / macOS users are advised to download the official desktop build from official channels; this project's Tauri shell is not the first choice; **mobile (iOS / Android): not supported**, because the architecture depends on a local Node service plus the system WebView, which does not fit mobile sandbox and background restrictions; only Linux is fully adapted and verified |
| Wayland window position | Determined by the compositor; `position()` only affects X11-style compositors |
| First-launch wait | Stage and elapsed time are visible, but there is no line-by-line npm output |

## 10. Notes

This project is unrelated to DeepSeek officially and contains no official binaries; the runtime is installed by the user with the commands above. The code is released under MIT, see [LICENSE](../LICENSE).
