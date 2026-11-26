# dsh-tauri

[简体中文](docs/DETAIL.zh.md) · [English](docs/DETAIL.en.md) · [Español](docs/DETAIL.es.md) · [Français](docs/DETAIL.fr.md) · [Deutsch](docs/DETAIL.de.md) · [Русский](docs/DETAIL.ru.md) · [Português](docs/DETAIL.pt.md)

用 **Tauri 2** 给 [DeepSeek Harness](https://github.com/deepseek-ai) 做的桌面客户端：
窗口、托盘、菜单和原生能力来自 Rust + 系统 WebView，Harness 本身运行官方 npm 包
`@deepseek-ai/dsh`，也就是 `dsh web` 那套纯 Node 服务。

**不需要 Electron，也不需要安装 DSH Desktop。** 一个 `node` 加一份
`node_modules/@deepseek-ai/dsh` 就是全部运行时。

![dsh-tauri 运行中的窗口](docs/images/shell.png)

## 和「把 Electron 应用塞进 Tauri」有什么不同

Tauri 只提供系统 WebView 和 Rust 后端，不带 Node 运行时。所以关键不在于换个壳，
而在于**跑哪个本体**：

| 做法 | 运行时 | 结果 |
| --- | --- | --- |
| Tauri 套 Electron 安装目录 | `/opt/DSH NEXT` 的 app.asar（内核依赖 Electron 主进程） | 仍然要对官方安装做逆向，官方一升级就崩 |
| **本项目** | npm 上的 `@deepseek-ai/dsh`，`dsh web` 一个普通 Node 进程 | 本体是公开分发的包，升级只需改版本号 |

于是外壳要做的事只剩三件：把服务拉起来、把窗口指过去、把浏览器给不了的能力补上。

## 架构

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

注入脚本（`src-tauri/src/native.rs::SHIM`）在页面脚本之前执行，补上
`__DSH_DIRECTORY_PICKER__`、`__DSH_TAURI__`（剪贴板/通知/外链），并把
`document.title` 同步到原生标题栏——WebKit 不会自己同步。

## 依赖

- Node 24+（在 PATH 里，或 `DSH_TAURI_NODE` 指定）
- Rust 1.9x、`webkit2gtk-4.1`、`libsoup-3.0`、`librsvg2`、`libayatana-appindicator3`、`patchelf`
- 运行时 Gnome 工具：`zenity`（目录选择）、`wl-clipboard`（剪贴板）、`libnotify-bin`（通知）

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf build-essential \
  zenity wl-clipboard libnotify-bin
```

## 构建与运行

```bash
npm install --prefix runtime @deepseek-ai/dsh   # 仅开发用：让 probe / 本仓库内启动拿到本体
npm test                                        # 运行时契约探针

cd src-tauri && cargo run
```

`cargo run` 会自己拉起服务并在窗口里打开。常用覆盖项：

| 变量 | 作用 |
| --- | --- |
| `DSH_TAURI_RUNTIME` | 运行时目录（含 `node_modules`） |
| `DSH_TAURI_NODE` | node 可执行文件 |
| `DSH_TAURI_PROFILE` | 启动哪个 profile（默认 `web`） |
| `DSH_HOME` | 数据目录（默认 `~/.dsh`） |
| `DSH_TAURI_URL` | 开发用：直接打开已在运行的服务，不自己 spawn |

打包（deb / AppImage）用 `npx tauri build`，不随包分发 `node_modules`：
**首次启动会自动安装**，把 `@deepseek-ai/dsh` 装进 `~/.dsh/dsh-tauri/runtime`，
期间 splash 会显示阶段（查找 / 安装 / 启动）与已用时，不需要你先跑任何命令。

外壳会按「`DSH_TAURI_RUNTIME` → 仓库内 `runtime/` → `~/.dsh/dsh-tauri/runtime`」依次查找。
只有在**离线**时才需要手动装一次：

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh
```

### 自动更新

服务起来之后，外壳会开一个后台线程跑 `npm view @deepseek-ai/dsh version`，
发现上游版本比本地新就自动装进同一个运行时目录，装完用 `notify-send`
提示「已更新到 <版本>，重启生效」。

- **不打断当前会话**：正在跑的服务不会被杀，旧目录改名为
  `<runtime>.old-<pid>` 继续供它读取，新版本**下次启动**才加载
- **失败静默重试**：检查或安装失败只写一行日志（`[runtime] 更新检查失败（下次启动再试）`），
  不弹窗、不影响使用，下次启动重新尝试
- **只更新自己装的目录**：`DSH_TAURI_RUNTIME` 指定的目录和仓库内 `runtime/`
  属于用户或 git 管理，外壳只记日志不写入
- **开关**（环境变量）：

  | 变量 | 作用 |
  | --- | --- |
  | `DSH_TAURI_AUTO_UPDATE=0` | 关掉版本检查，首次启动的自动安装照常 |
  | `DSH_TAURI_OFFLINE=1` | 关掉全部网络行为（安装 + 检查），离线也能启动已有运行时 |

手动升级仍然可行，等价于让外壳不做：

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh@latest
node tools/probe-web.mjs --runtime ~/.dsh/dsh-tauri/runtime   # 升级后先验契约
```

## 目录

```
src-tauri/src/dsh.rs        服务生命周期：spawn、解析 URL、崩溃重启、优雅退出
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

## 能力对照

| 能力 | 实现方式 |
| --- | --- |
| 会话 / 工作区 / 插件 | 官方 Web UI，加载自 `dsh web` |
| 目录选择、剪贴板、通知、外链 | 注入脚本 + Tauri 命令 |
| 窗口标题、尺寸、托盘、单实例 | `shell_ui.rs` / `single_instance.rs` |
| 崩溃自动重启 | `dsh.rs`，最多 3 次后错误页 |
| 运行时安装/更新 | 自动（首次启动安装，启动后自动检查更新） |
| 启动进度 | splash 轮询 `runtime_status`：阶段 + 已用时 |

## 安全与健壮性

- 启动 URL 与外链 URL 都做了严格校验：只接受 loopback / 白名单 scheme，拒绝控制字符；
- 运行时安装/更新走暂存 + 原子替换；进程登记表用 RAII 保证注销，退出时按进程组清理；
- 单实例 socket、窗口状态写入、陈旧运行时清理都做了防误删/防竞态加固；
- 上游版本号严格按 semver 解析，npm 错误文本不会被当成版本；
- 桌面桥接契约与「dshDesktop 不实现」决策见 `docs/DESKTOP_BRIDGE.zh.md`。

## 已验证（Linux + WebKitGTK 2.52）

- `dsh web` 由外壳拉起，从 stdout 解析出 `http://127.0.0.1:<port>/?token=…`；
  首屏在 3–30 秒内出现（取决于缓存）；
- 认证链路：token URL → 303 + `dsh-auth-*` cookie → `GET /` 返回客户端 HTML（`tools/probe-web.mjs`）；
- 窗口里是完整 Web UI：会话列表、工作区、模型选择、插件页、正在进行的会话流都在；
- 窗口会随 `document.title` 改名；关闭按偏好走「关到托盘」或「退出」；重启恢复尺寸；
- 第二次启动会聚焦已有窗口后退出，不会起第二个服务；
- 服务被杀后自动重启（最多 3 次），仍失败则显示可读的错误页；
- 首次启动没有运行时会自动 `npm install` 到 `~/.dsh/dsh-tauri/runtime`，
  splash 显示当前阶段与已用时；
- 装完自动起服务；有上游新版时后台安装并用 `notify-send` 提示「重启生效」，
  正在跑的服务不受影响；
- `DSH_TAURI_OFFLINE=1` 且本地无运行时，显示带手动命令的错误页而不是空白窗口；
- 更新走 `<runtime>.new-<pid>` 暂存目录 + 原子替换，安装中断不会破坏已装好的运行时。
- 三层自动验证：`npm test`（真运行时契约探针）、`npm run test:unit`（注入脚本、CLI、错误页与启动页单测，25 项）、`npm run test:contract`（上游桥接漂移守卫，13 项静态检查（含 SHIM 双窗口注入点、withGlobalTauri 与 dshDesktop 不实现检查））；Rust 单测 135 项；CI 全部执行。

## 已知限制

| 项 | 现状 |
| --- | --- |
| 平台页内嵌视图（用量 / 充值） | 走浏览器打开，未做原生内嵌 |
| 更新后的旧运行时占盘 | 旧树改名 `<runtime>.old-<pid>` 保留给仍在跑的服务，**到下次启动才清理**；期间多占约一份运行时的空间（约 1.2G） |
| SIGKILL 残留 | 外壳被 `SIGKILL` 时来不及收子进程，npm 的 `postinstall` 孙进程可能短暂残留；正常退出（SIGTERM）不受影响 |
| Windows / macOS | 代码按 Linux 原生命令编写（zenity/wl-clipboard/notify-send），其它平台需要替换这几处 |
| Wayland 窗口位置 | 由合成器决定，`position()` 只影响 X11 风格合成器 |
| 首启动等待 | 能看阶段与已用时，但没有逐行 npm 输出 |

## 说明

本项目与 DeepSeek 官方无关，不包含官方二进制；运行时由用户按上面的命令安装。
代码以 MIT 发布，见 [LICENSE](LICENSE)。

## 支持本项目

如果这个项目对你有帮助，欢迎给 [pengtang1024/dsh-tauri](https://github.com/pengtang1024/dsh-tauri) 点个 star ⭐，你的支持是我继续维护的动力。
