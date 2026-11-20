# dsh-tauri

用 **Tauri 2** 重做的 DeepSeek Harness Desktop 外壳：窗口与原生能力由 Rust + 系统 WebView 提供，
内核仍然是官方那套 Node `@deepseek-ai/*` 运行时——不重写它，也不复制它。

它启动官方安装里自带的 Host 进程（纯 Node），
在 Node 侧做协议桥接与会话认证，再把页面交给系统 WebView 渲染。
Electron 那 228 MB 的 Chromium 外壳不再需要，而**能力仍在**：桌面模式、实时流、目录选择器、
剪贴板、通知、外链跳转都通过同一套接口对外提供。

![DSH Tauri 外壳](docs/images/shell.png)

> 状态：**v0.1，可用**。Linux + WebKitGTK 已实测跑通（窗口、桌面模式、实时 WebSocket 流、
> 原生桥接口）。发布形态、托盘、原生菜单仍在路线图上，见 [已知差距](#已知差距)。

## 为什么不是「打包 Electron 应用」

Tauri 只提供系统 WebView 与 Rust 后端，**不带 Node 运行时**；而 DSH 的内核是一组 Node 服务。
所以真正的移植点是「外壳」，不是「内核」。本项目把外壳拆成两半：

- **Rust（`src-tauri/`）**：窗口、生命周期、未来的托盘/菜单/原生对话框；
- **Node（`bridge/`）**：`fork` 官方 `lib/host.js`，把它的 IPC 通道转成换行分隔的 JSON；
  同时在本机回环地址上跑一个反向代理，替渲染层补上会话 cookie 与渲染令牌。

被移除的是 Chromium 自带的渲染进程与 Electron 主进程；被保留的是官方安装里的 Host、插件、
profile 与会话数据——因此**与官方桌面共用同一份 `~/.dsh`**，会话、插件、账号状态都一致。

## 架构

```
┌──────────────────────────── Tauri 进程（Rust） ────────────────────────────┐
│  WebviewWindow ──► http://127.0.0.1:<proxy>/                              │
│  监督 bridge 子进程：读取 JSON 事件，收到 ready 后开窗                      │
└───────────────▲───────────────────────────────────────────────────────────┘
                │ stdin/stdout：每行一个 JSON
┌───────────────┴──── bridge（Node，`bridge/host-bridge.mjs`）──────────────┐
│ 1. spawn `<root>/lib/host.js`，stdio: [ignore, pipe, pipe, ipc]            │
│ 2. 用 x-dsh-desktop-renderer 令牌换 303 + 会话 cookie                       │
│ 3. 起回环反代：补 cookie/令牌/Origin，转发 HTTP 与 WebSocket 升级           │
│ 4. 注入 renderer shim：重建 window.dshDesktop* / 目录选择器等全局           │
│ 5. 暴露 /__dsh_tauri__/ 原生 API（目录选择、剪贴板、通知、外链、权限）      │
└───────────────▲───────────────────────────────────────────────────────────┘
                │ 官方 Host 的 fork IPC（fd 3）
┌───────────────┴──────────── Host（官方 Node 运行时，未修改）──────────────┐
└───────────────────────────────────────────────────────────────────────────┘
```

完整的逆向结论（进程参数、认证握手、需要剔除的响应头、WebSocket 约束）记录在
[docs/CONTRACT.md](docs/CONTRACT.md)，可直接用 `tools/probe-host.mjs` 复现。

## 依赖

- 已安装的 **DSH Desktop**（默认探测 `/opt/DSH NEXT/resources/app.asar`，
  也可用 `DSH_TAURI_APP_ROOT` 指定）
- Rust 1.9x、`webkit2gtk-4.1`、`libsoup-3.0`、`librsvg2`、`libayatana-appindicator3`、`patchelf`
- Node ≥ 22（只用来跑 bridge；也可以用官方安装自带的 Node）

Debian/Ubuntu：

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf build-essential
```

## 构建与运行

```bash
# 1) 开发运行
cd src-tauri
cargo run

# 2) 只验证 Host 契约（不启动窗口）
cd ..
node tools/probe-host.mjs      # 直连 Host：认证 / 反代 / 注入
node tools/probe-bridge.mjs    # 完整链路：bridge + 回环反代 + WebSocket
```

测试与桥接自检：

```bash
npm test                       # shell API 路由测试（stage-file / 404 / 前缀隔离）
npm run check                  # 三个 bridge 脚本的语法检查
node tools/probe-bridge.mjs    # 真实拉起 Host，验证认证、反代与 WebSocket
```

`cargo run` 会自动发现官方安装；如需指定：

```bash
DSH_TAURI_APP_ROOT="/opt/DSH NEXT/resources/app.asar" cargo run
```

其它可覆盖项：`DSH_TAURI_NODE`、`DSH_TAURI_BRIDGE`、`DSH_TAURI_PROFILE`、
`DSH_TAURI_PRIMARY_RUNTIME`、`DSH_TAURI_EXECUTABLE`、`DSH_TAURI_SHIM`。

## 目录

```
bridge/host-bridge.mjs    Host 监督 + 回环反代 + 行式 JSON 桥
bridge/shell-api.mjs      /__dsh_tauri__ 原生 API（目录选择、剪贴板、通知、外链、权限）
bridge/renderer-shim.js   注入页面的 renderer 全局（重建 Electron preload 的公开面）
src-tauri/src/bridge.rs   子进程监督、安装探测、ready 后开窗
src-tauri/src/main.rs     Tauri 应用与退出清理
tools/                    契约探针与 asar 提取工具
docs/CONTRACT.md          逆向出来的官方外壳契约
```

## 已验证

- 官方 Host 以 `ELECTRON_RUN_AS_NODE` 方式启动并回报 `ready`（14 项 index 注入）；
- 认证：`x-dsh-desktop-renderer` → 303 + `set-cookie`；裸 cookie 或裸令牌均被拒（403 / 401）；
- 反向代理：`GET /` 200 且是真客户端 HTML；`/plugins/**` 不缓存；
- WebSocket：`/api/remote.mux` 经代理返回 **101** 并保持长连接（会话与终端流实时刷新）；
- 桌面模式：`dshDesktopBoot.ready()` 打通，`__DSH_TRANSPORT__.ownsHost` 生效；
- 单实例：第二次启动会把已有窗口提到前台后退出（`$XDG_RUNTIME_DIR/dsh-tauri.sock`），
  不会在同一 profile 上再起第二个 Host；
- Host 意外退出（被杀、崩溃）时自动重启，最多 3 次，仍失败则弹错误窗口说明原因；
- 平台登录：Host 发来 `platform-login{action:open}` 时开登录窗；登录窗跳回 Host 的
  `/oauth/callback` 时由 bridge 以原生渲染器身份经代理重放（实测回调 404 → 窗口自动关闭）；
- 窗口标题跟随文档标题（`document.title` → 原生标题栏，实测从 `DSH` 变为 `DeepSeek Harness`）；
- 原生 API：`/__dsh_tauri__/info`、`/preferences`、`/permissions` 实测返回；
  目录选择走 zenity，剪贴板走 wl-copy/wl-paste，通知走 notify-send，外链走 xdg-open。

## 已知差距

官方 Electron 外壳里还有一批只有预加载脚本才能提供的入口，本项目按需补齐：

| 能力 | 现状 |
| --- | --- |
| 托盘 / 关闭到托盘 | 已实现：托盘菜单「显示主窗口 / 退出」，`closeToTray` 为真时关闭窗口只隐藏 |
| 原生菜单栏 | 已实现：编辑 / 查看 / 窗口（GTK 菜单，随窗口显示） |
| 拖拽文件取绝对路径（`__DSH_HOST_PATHS__`） | 已实现：落点被拦截后把文件复制到 `~/.dsh/dsh-tauri-drops/` 并回传绝对路径；复制而非原地引用，文件后续改动不会同步 |
| 打包（.deb / AppImage / 图标 / .desktop） | 已实现：`npm run build` 产出 `.deb` 与 AppImage，CI 上传为构建产物 |
| 平台页内嵌视图（用量 / 充值，`dshPlatform`） | 未实现：账号额度页的「去充值」入口在桌面模式下被客户端隐藏，需在网页端操作 |
| 桌面引导 / 首次运行迁移（`dshDesktop`、`dshDesktopSetup`） | 未实现：登录走网页流程，客户端有回退路径 |
| 应用内账号子窗（`dshPlatform` + 凭据注入） | 未实现，见上一条 |
| 自动更新 | 未实现，建议随官方安装一起升级 |
| 桌面引导 / 迁移（`dshDesktopSetup`） | 未实现；已有 profile 直接可用，迁移流程建议在官方桌面完成一次 |

## 说明

本项目与 DeepSeek 官方无关，不含官方二进制；运行期复用**用户本机已安装**的 DSH Desktop
运行时。上游源码仅用于阅读契约，不随本仓库分发。代码以 MIT 发布，见 [LICENSE](LICENSE)。
