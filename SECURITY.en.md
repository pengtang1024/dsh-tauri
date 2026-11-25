# Security Policy

## Supported versions

| Version | Receives security fixes |
| --- | --- |
| `0.4.x` (the currently maintained line, starting from `0.4.0`) | Yes |
| `0.3.x` and earlier | No — please upgrade to the latest supported version first |

Only the `0.4.x` line is currently maintained. Issues in older versions that still exist in the new version are fixed there; no separate patches are released for old versions.

## Private reporting

**Do not open a public issue, PR or discussion for a security vulnerability**, and do not post exploitable details in public channels. Use either of these private routes:

1. GitHub Security Advisories (preferred): <https://github.com/pengtang1024/dsh-tauri/security/advisories/new>
2. Email: <pengtang1024@qq.com>, preferably with `[SECURITY]` in the subject line.

Please include as much of the following as you can in your report:

- The affected specific `0.4.x` version, and your OS / WebKitGTK version.
- Reproduction steps or a minimal PoC.
- The actual impact (e.g. whether a session token can be read, arbitrary commands executed, local files opened without authorization, or the runtime directory corrupted).
- If convenient, a suggested fix.

## Response timeline

- **Within 3 business days**: acknowledge the report and give an initial assessment.
- Once confirmed as a valid issue, we will outline a fix plan; after the fix is released, the GitHub Security Advisory will record the affected and fixed versions.
- If the report is judged out of scope, we will explain why and point you to the appropriate upstream channel.

## Scope

This repository is responsible for security issues in **the desktop shell itself**:

- Locating, spawning, URL parsing and crash restart of the `dsh web` service in `src-tauri/src/dsh.rs`;
- Runtime discovery, installation, version comparison and atomic swap in `src-tauri/src/runtime.rs`, `install.rs`, `update.rs` and `swap.rs`;
- Native commands and the injected script (`SHIM`) in `src-tauri/src/native.rs`, including clipboard, notifications, directory picker and external-link opening;
- Window/tray, single-instance guard, child-process cleanup and startup-log parsing in `src-tauri/src/shell_ui.rs`, `single_instance.rs`, `exec.rs`, `banner.rs`, `progress.rs`, `version.rs` and `supervisor.rs`;
- The `tools/probe-web.mjs` runtime contract probe, and the build/release workflows under `.github/workflows/`.

**Out of scope for this repository**: the official `@deepseek-ai/dsh` npm package itself (including the Web UI, plugins, provider routing and credential storage), upstream components such as Tauri / WebKitGTK / wl-clipboard, system package managers and distribution configuration, model services and platform accounts. Please route such issues through the appropriate upstream channels:

- <https://github.com/deepseek-ai/deepseek-harness/security>
- Or any other security reporting entry published by the corresponding upstream repository / official site.

The root `package.json` declares only the dev dependency `@tauri-apps/cli`, and `runtime/package.json` is a local development install directory; Dependabot checks both npm locations plus GitHub Actions weekly. Released packages do not bundle `node_modules`: the shell installs the runtime into a user directory on first launch, pulling from npm with the user's own credentials.

## Security design notes

- **Session tokens only travel through a loopback URL.** The shell parses `http://127.0.0.1:<port>/?token=…` from `dsh web`'s stdout and points the window at it; the token is never written into the repository, logs or configuration files. Logs record only parse failures and stage information, not the full URL.
- **External links are allow-listed.** `open_external` in `native.rs` accepts only `http(s)` and `mailto` and rejects other schemes, so remote content in the renderer cannot use `xdg-open` to trigger local programs. Capabilities such as the directory picker and clipboard are exposed only through explicit Tauri commands.
- **Child processes are never built from concatenated command strings.** npm, node, zenity, wl-copy and others are launched with `Command::new(program)` plus separate arguments, never through a shell string. Helpers carry timeouts and are killed as a process group when the shell exits (`PR_SET_PDEATHSIG` included), leaving no strays.
- **Runtime updates are atomic.** Installation writes a `<runtime>.new-<pid>` staging directory first and only renames it into place on success; a failure or interruption cannot corrupt an already installed runtime. `.old-<pid>` trees remain readable by a running service and are cleaned up on the next launch.
- **Network activity can be turned off.** `DSH_TAURI_OFFLINE=1` disables installation and version checks; `DSH_TAURI_AUTO_UPDATE=0` disables only version checks. Directories selected via `DSH_TAURI_RUNTIME` and the in-repo `runtime/` belong to the user or git, so the shell only logs about them and never writes to them.
- **A single instance prevents duplicate services.** A second launch focuses the existing window via a Unix socket and exits instead of starting a second Host service on the same profile.

## Known boundaries

- The token in the loopback URL is the credential for the Web UI. Even though the service listens only locally, do not paste a full URL containing `token=` into issues, logs or conversations.
- The `node_modules` tree in the runtime directory is a plaintext npm installation. Install from the official npm registry, never a package of unknown origin, and make sure any directory you point `DSH_TAURI_RUNTIME` at is trustworthy.
- The shell does not validate the content served by `DSH_TAURI_URL`: this development variable connects directly to an already-running service, so point it only at a local instance you trust.
- Automatic updates make the next launch load a newer package. If that is unacceptable in a managed or restricted environment, set `DSH_TAURI_OFFLINE=1` or `DSH_TAURI_AUTO_UPDATE=0` explicitly.
- The project is currently written for Linux / Wayland + WebKitGTK (zenity, wl-clipboard, notify-send); native-command behavior on other platforms has not been security-audited.
