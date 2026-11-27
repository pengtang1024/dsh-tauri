# dsh-tauri 详细说明

> 本文档根据仓库中的 `README.md` 与 `docs/DEVELOPMENT.md` 忠实整理，供快速了解项目定位、架构、依赖与安装、配置、用法、验证命令以及已知限制/边界。代码块、标识符、数字、路径与命令均保持原样。

## 1. 项目定位

用 **Tauri 2** 给 [DeepSeek Harness](https://github.com/deepseek-ai) 做的桌面客户端：窗口、托盘、菜单和原生能力来自 Rust + 系统 WebView，Harness 本身运行官方 npm 包 `@deepseek-ai/dsh`，也就是 `dsh web` 那套纯 Node 服务。

**不需要 Electron，也不需要安装 DSH Desktop。** 一个 `node` 加一份 `node_modules/@deepseek-ai/dsh` 就是全部运行时。

### 核心目标

**目标层**：本项目以 Linux 为第一优先、深度支持——优先支持各种发行版与不同版本，在兼容性、运行性能与资源消耗上尽量做到最好，Linux 是唯一完成适配与验证的平台；Windows / macOS 提供桌面端基础支持——能构建、能运行即可，不做深入适配与优化，建议直接从官方渠道下载 DeepSeek Harness 官方桌面版本，本项目的 Tauri 壳不作为 Win/Mac 用户的首选方案；移动端（iOS / Android）不支持——架构上依赖本地 Node 服务 + 系统 WebView，移动端沙箱与后台限制不适用。

> **现状层（如实说明）**：Linux 是唯一完成适配与验证的平台（当前代码依赖 Linux 原生命令 `zenity` / `wl-clipboard` / `notify-send`，且只在 Linux + WebKitGTK 2.52 上验证过；多发行版兼容仍是目标，不代表已全面验证）。Windows / macOS 提供基础支持、能构建能运行，但不做深入适配与优化，未做全面验证；移动端（iOS / Android）明确不支持，原因是架构上依赖本地 Node 服务 + 系统 WebView，移动端沙箱与后台限制不适用。

### 和「把 Electron 应用塞进 Tauri」有什么不同

Tauri 只提供系统 WebView 和 Rust 后端，不带 Node 运行时。所以关键不在于换个壳，而在于**跑哪个本体**：

| 做法 | 运行时 | 结果 |
| --- | --- | --- |
| Tauri 套 Electron 安装目录 | `/opt/DSH NEXT` 的 app.asar（内核依赖 Electron 主进程） | 仍然要对官方安装做逆向，官方一升级就崩 |
| **本项目** | npm 上的 `@deepseek-ai/dsh`，`dsh web` 一个普通 Node 进程 | 本体是公开分发的包，升级只需改版本号 |

于是外壳要做的事只剩三件：把服务拉起来、把窗口指过去、把浏览器给不了的能力补上。

## 2. 架构

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

注入脚本（`src-tauri/src/native.rs::SHIM`）在页面脚本之前执行，补上 `__DSH_DIRECTORY_PICKER__`、`__DSH_TAURI__`（剪贴板/通知/外链），并把 `document.title` 同步到原生标题栏——WebKit 不会自己同步。

### 分层

| 层 | 位置 | 职责 |
| --- | --- | --- |
| 服务监督 | `src-tauri/src/supervisor.rs` | spawn `dsh web`、从 stdout 解析 URL、崩溃重启、SIGTERM 退出 |
| 原生能力 | `src-tauri/src/native.rs` | 目录选择、剪贴板、通知、外链、窗口标题 + 注入脚本 |
| 外壳 | `src-tauri/src/shell_ui.rs` | 窗口、菜单、托盘、窗口状态、错误页、几何裁剪 |
| 单实例 | `src-tauri/src/single_instance.rs` | `$XDG_RUNTIME_DIR/dsh-tauri.sock` |

### 目录

```
src-tauri/src/main.rs      应用入口：Tauri builder、命令注册、启动流程与关停事件
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

### 能力对照

| 能力 | 实现方式 |
| --- | --- |
| 会话 / 工作区 / 插件 | 官方 Web UI，加载自 `dsh web` |
| 目录选择、剪贴板、通知、外链 | 注入脚本 + Tauri 命令 |
| 窗口标题、尺寸、托盘、单实例 | `shell_ui.rs` / `single_instance.rs` |
| 崩溃自动重启 | `supervisor.rs`，最多 3 次后错误页 |
| 运行时安装/更新 | 自动（首次启动安装，启动后自动检查更新） |
| 启动进度 | splash 轮询 `runtime_status`：阶段 + 已用时 |

## 3. 依赖与安装

- Node 24+（在 PATH 里，或 `DSH_TAURI_NODE` 指定）
- Rust 1.9x、`webkit2gtk-4.1`、`libsoup-3.0`、`librsvg2`、`libayatana-appindicator3`、`patchelf`
- 运行时助手：`zenity`（目录选择 / 关于面板）、`wl-clipboard`（剪贴板）、`libnotify-bin`（`notify-send` 通知）、`xdg-utils`（`xdg-open` 外链）
- Debian / Ubuntu 的 `.deb` 包已在 `bundle.linux.deb.depends` 中声明这 4 个助手，安装时一并拉取，开箱即用；AppImage 与源码运行需自行安装：

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf build-essential \
  zenity wl-clipboard libnotify-bin xdg-utils
```

## 4. 构建与用法

```bash
npm install --prefix runtime @deepseek-ai/dsh   # 仅开发用：让 probe / 本仓库内启动拿到本体
npm test                                        # 运行时契约探针

cd src-tauri && cargo run
```

`cargo run` 会自己拉起服务并在窗口里打开。

打包（deb / AppImage）用 `npx tauri build`，不随包分发 `node_modules`：**首次启动会自动安装**，把 `@deepseek-ai/dsh` 装进 `~/.dsh/dsh-tauri/runtime`，期间 splash 会显示阶段（查找 / 安装 / 启动）与已用时，不需要你先跑任何命令。

外壳会按「`DSH_TAURI_RUNTIME` → 仓库内 `runtime/` → `~/.dsh/dsh-tauri/runtime`」依次查找。只有在**离线**时才需要手动装一次：

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh
```

### 自动更新

服务起来之后，外壳会开一个后台线程跑 `npm view @deepseek-ai/dsh version`，发现上游版本比本地新就自动装进同一个运行时目录，装完用 `notify-send` 提示「已更新到 <版本>，重启生效」。

- **不打断当前会话**：正在跑的服务不会被杀，旧目录改名为 `<runtime>.old-<pid>` 继续供它读取，新版本**下次启动**才加载
- **失败静默重试**：检查或安装失败只写一行日志（`[runtime] 更新检查失败（下次启动再试）`），不弹窗、不影响使用，下次启动重新尝试
- **只更新自己装的目录**：`DSH_TAURI_RUNTIME` 指定的目录和仓库内 `runtime/` 属于用户或 git 管理，外壳只记日志不写入

手动升级仍然可行，等价于让外壳不做：

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh@latest
node tools/probe-web.mjs --runtime ~/.dsh/dsh-tauri/runtime   # 升级后先验契约
```

### 运行时查找顺序

`DSH_TAURI_RUNTIME` → 仓库内 `runtime/`（存在 `node_modules` 才算）→ `~/.dsh/dsh-tauri/runtime` → 打包资源目录下的 `runtime/`。

都找不到时显示错误页，正文里带安装命令。

`runtime::Source` 记录命中哪一档，只有 `Home` 和 `Resource` 是外壳自己的目录，自动更新可以写入；`Explicit`（`DSH_TAURI_RUNTIME`）和 `Checkout`（仓库内）只打日志，绝不动——那里可能是 git 跟踪的文件。

### 首次启动自动安装

`runtime::ensure` 在四档都找不到运行时、且不是离线模式时调用 `install::install(dir, "latest")`，装到 `~/.dsh/dsh-tauri/runtime`：

```
runtime::ensure          定位失败 → 非离线 → install_fresh
  install::npm_available 先跑 `npm --version`，缺 npm 给可读错误
  swap::Staging::new     建 <runtime>.new-<pid> 暂存目录（先清掉旧的临时树）
  exec::run              npm install --prefix <staging> --no-audit --no-fund
                          --loglevel=error @deepseek-ai/dsh@latest（600 秒超时）
  has_package            校验 node_modules/@deepseek-ai/dsh/lib/bin.js 存在
  Staging::commit        rename 换上去，旧的改名 .old-<pid>
```

- 安装期间 `progress::set(Stage::Installing, …)`，splash 轮询 `runtime_status` 显示阶段与已用时；
- `exec.rs` 让 npm 跑在自己的会话里，超时或退出时按进程组整组收，不留 postinstall 孙进程；
- 任何一步失败都返回可读错误 + 手动命令，走 `progress::fail` → 错误页。

### 自动更新实现

服务启动后 `update::spawn_once(dir, source)` 起一条 `dsh-update` 线程（进程内只跑一次）：

1. `version_at(dir)` 读本地 `node_modules/@deepseek-ai/dsh/package.json` 的 version；
2. `npm view @deepseek-ai/dsh version` 拿上游版本（90 秒超时，遵循用户的 npm 镜像配置）；
3. `version::is_newer` 比较（纯 Rust 语义化比较，含 prerelease 规则，见 `version.rs` 单测）；
4. 更新则 `progress::set(Stage::Updating, …)` → `install::install(dir, "latest")` 走同一套暂存 + 原子替换 → `notify-send -a DSH "DSH 已更新" "已更新到 x.y.z，重启生效"`。

约束：不杀正在跑的服务（新版本下次启动才加载）、失败只打 `[runtime] 更新检查失败（下次启动再试）` 并在下次启动重试、`Source::updatable()` 为假时只记一行日志。

### 原子替换与清理

- 暂存目录 `<runtime>.new-<pid>`、旧树 `<runtime>.old-<pid>` 都是运行时的**同级兄弟目录**，保证 `rename` 同文件系统、原子；
- `commit` 先把旧目录 rename 成 `.old-<pid>`，再把暂存 rename 成正式目录；第二步失败会把旧目录 rename 回来（错误信息里写明「旧运行时已恢复」）；
- `Staging::drop` 在没 commit 时删掉暂存树 —— 安装中断不会留半成品；
- 旧树**留给仍在跑的服务**（它的模块已从旧树加载），因此更新后磁盘上会同时存在两份（约 1.2G）；`swap::clean_stale_runtime()` 在 `main.rs` 的 `.setup` 里、单实例抢到之后调用，清掉同级目录中所有 `<name>.new-*` / `<name>.old-*`，失败只打日志。

## 5. 配置与环境变量

`cargo run` 会自己拉起服务并在窗口里打开。常用覆盖项：

| 变量 | 作用 |
| --- | --- |
| `DSH_TAURI_RUNTIME` | 运行时目录（含 `node_modules`） |
| `DSH_TAURI_NODE` | node 可执行文件 |
| `DSH_TAURI_HOST` | 服务绑定主机，仅接受 loopback（默认 `127.0.0.1`，可设 `localhost` / `::1`） |
| `DSH_TAURI_NPM` | npm 可执行文件路径（默认 PATH 中的 npm；自动安装/更新与版本探测都用它） |
| `DSH_TAURI_PROFILE` | 启动哪个 profile（默认 `web`） |
| `DSH_HOME` | 数据目录（默认 `~/.dsh`） |
| 窗口状态文件 | `~/.dsh/dsh-tauri-window-state.json`：记录窗口几何（尺寸、位置、最大化状态），下次启动据此恢复；不随 `DSH_HOME` 改变 |
| `DSH_TAURI_URL` | 开发用：直接打开已在运行的 loopback 服务（仅 http(s)+127.0.0.1/localhost/[::1]），不自己 spawn |

### 自动更新开关（环境变量）

| 变量 | 作用 |
| --- | --- |
| `DSH_TAURI_AUTO_UPDATE=0` | 关掉版本检查，首次启动的自动安装照常 |
| `DSH_TAURI_OFFLINE=1` | 关掉全部网络行为（安装 + 检查），离线也能启动已有运行时 |

### 两个开关（取值与效果）

| 变量 | 取值 | 效果 |
| --- | --- | --- |
| `DSH_TAURI_OFFLINE` | `1/true/yes/on` | 关掉安装与检查（全部联网行为）；无运行时则错误页 |
| `DSH_TAURI_AUTO_UPDATE` | `0/false/no/off` | 只关版本检查，首次启动的自动安装照常 |

实现见 `runtime.rs::offline()` / `auto_update_enabled()`；`auto_update_enabled` 先把 `OFFLINE` 短路掉，所以离线时两者都关。

## 6. 关键实现注意点

1. **服务 URL 只从 stdout 拿**：`dsh web` 打印 `dsh web: http://127.0.0.1:<port>/?token=…`；不要自己猜端口，`--port 0` 让系统分配。
2. **窗口必须在拿到 URL 之后直接创建在该 URL 上**。先建 splash 窗口再 `navigate` 会让 WebKit 丢掉认证重定向的 `Set-Cookie`（表现为页面只剩 `dsh web authentication required`）。所以用独立 label 的 splash 窗口，命中 URL 时另建 `main` 窗口再销毁 splash。
3. **注入脚本要在页面脚本之前**：`WebviewWindowBuilder::initialization_script`。它依赖 `withGlobalTauri`（见 `tauri.conf.json`）提供的 `window.__TAURI__.core.invoke`。
4. **查询字符串里的 token 是一次性认证入口**：`GET` 返回 303 并下发 `dsh-auth-<authority>` cookie（`HttpOnly; SameSite=Strict`）。服务端按 authority 生成 cookie 名，端口变了名字就变。
5. **退出走 SIGTERM**：`dsh::shutdown()` 先 `kill -TERM`，5 秒后仍未退出才 SIGKILL，避免会话库留下 WAL 残渣。
6. **托盘退出**要先 `dsh::shutdown()` 再 `app.exit(0)`，否则会留下孤儿 node 进程。

## 7. 验证命令

```bash
node --check tools/probe-web.mjs
npm test                                     # 拉起 dsh web，验证 303 + cookie + 客户端 HTML
cd src-tauri && cargo test && cargo build    # Rust 单测 + 构建
```

`npm test` 需要先装运行时：

```bash
npm install --prefix runtime @deepseek-ai/dsh
```

### 调试自动安装与更新

**假 npm**（不联网、不下载 1.2G，验证整条安装/更新链路）：`install::npm()` 认 `DSH_TAURI_NPM`，写一个可执行脚本冒充 npm 即可。

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

- **首次启动安装**：把 `~/.dsh/dsh-tauri/runtime` 挪走（`mv` 到 `/tmp`），`DSH_TAURI_NPM=/tmp/fake-npm cargo run`，应看到 splash 进入「正在安装」阶段，结束后 `~/.dsh/dsh-tauri/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js` 存在；但假包没有真服务，窗口最终会停在启动失败——验证安装链路足够。
- **更新路径**：装好后让 `FAKE_LOCAL=1.0.0`、`FAKE_UPSTREAM=2.0.0` 再启动，日志应出现 `发现新版 2.0.0（本地 1.0.0）`、`已原子替换`、`旧运行时保留至下次启动清理`，同级目录出现 `<runtime>.old-<pid>`；再启动一次应打印 `清理上次运行留下的 …`。
- **开关**：`DSH_TAURI_OFFLINE=1` 且无运行时 → 错误页正文带 `npm install --prefix` 命令；`DSH_TAURI_AUTO_UPDATE=0` → 日志出现 `自动更新已关闭`，但首次安装仍会执行。
- **安装失败**：让假 npm 的 `install` 分支 `exit 1` → 错误页带 npm 输出与手动命令，且原运行时不受影响（`<runtime>.new-<pid>` 已被 `Drop` 清掉）。
- **收尾**：`pkill -x dsh-tauri`；确认 `~/.dsh/dsh-tauri/` 下没有 `.new-*` / `.old-*` 残留，假 npm 与挪走的 runtime 记得删掉/放回。

### 冒烟检查（人工）

```bash
cd src-tauri && cargo run          # 应看到 splash，随后出现完整 UI
curl -s -o /dev/null -w '%{http_code}\n' "$(grep -ao 'http://127.0.0.1:[0-9]*/?token=[A-Za-z0-9_-]*' /tmp/xxx.log | head -1)"
```

窗口内应能看到：会话列表、工作区、模型选择器、正在运行的会话流。

## 8. 已验证（Linux + WebKitGTK 2.52）

- `dsh web` 由外壳拉起，从 stdout 解析出 `http://127.0.0.1:<port>/?token=…`；首屏在 3–30 秒内出现（取决于缓存）；
- 认证链路：token URL → 303 + `dsh-auth-*` cookie → `GET /` 返回客户端 HTML（`tools/probe-web.mjs`）；
- 窗口里是完整 Web UI：会话列表、工作区、模型选择、插件页、正在进行的会话流都在；
- 窗口会随 `document.title` 改名；关闭按偏好走「关到托盘」或「退出」（偏好在 `~/.dsh/desktop-preferences.json` 的 `closeToTray` 字段：`true`=关到托盘，`false`=退出，缺省 `true`）；重启恢复尺寸；
- 第二次启动会聚焦已有窗口后退出，不会起第二个服务；
- 服务被杀后自动重启（最多 3 次），仍失败则显示可读的错误页；
- 首次启动没有运行时会自动 `npm install` 到 `~/.dsh/dsh-tauri/runtime`，splash 显示当前阶段与已用时；
- 装完自动起服务；有上游新版时后台安装并用 `notify-send` 提示「重启生效」，正在跑的服务不受影响；
- `DSH_TAURI_OFFLINE=1` 且本地无运行时，显示带手动命令的错误页而不是空白窗口；
- 更新走 `<runtime>.new-<pid>` 暂存目录 + 原子替换，安装中断不会破坏已装好的运行时。

## 9. 已知限制 / 边界

| 项 | 现状 |
| --- | --- |
| 平台页内嵌视图（用量 / 充值） | 走浏览器打开，未做原生内嵌 |
| 更新后的旧运行时占盘 | 旧树改名 `<runtime>.old-<pid>` 保留给仍在跑的服务，**到下次启动才清理**；期间多占约一份运行时的空间（约 1.2G） |
| SIGKILL 残留 | 外壳被 `SIGKILL` 时来不及收子进程，npm 的 `postinstall` 孙进程可能短暂残留；正常退出（SIGTERM）不受影响 |
| Windows / macOS / 移动端（iOS / Android） | **Windows / macOS 基础支持**：能构建、能运行即可，不做深入适配与优化，未做全面验证；建议 Win/Mac 用户直接从官方渠道下载 DeepSeek Harness 官方桌面版本，本项目 Tauri 壳不作为首选方案；**移动端（iOS / Android）不支持**：架构上依赖本地 Node 服务 + 系统 WebView，移动端沙箱与后台限制不适用；仅 Linux 完成适配与验证 |
| Wayland 窗口位置 | 由合成器决定，`position()` 只影响 X11 风格合成器 |
| 首启动等待 | 能看阶段与已用时，但没有逐行 npm 输出 |

## 10. 说明

本项目与 DeepSeek 官方无关，不包含官方二进制；运行时由用户按上面的命令安装。代码以 MIT 发布，见 [LICENSE](../LICENSE)。
