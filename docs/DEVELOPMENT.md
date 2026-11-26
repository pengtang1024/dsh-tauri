# 开发手册

## 分层

| 层 | 位置 | 职责 |
| --- | --- | --- |
| 服务监督 | `src-tauri/src/dsh.rs` | spawn `dsh web`、从 stdout 解析 URL、崩溃重启、SIGTERM 退出 |
| 原生能力 | `src-tauri/src/native.rs` | 目录选择、剪贴板、通知、外链、窗口标题 + 注入脚本 |
| 外壳 | `src-tauri/src/shell_ui.rs` | 窗口、菜单、托盘、窗口状态、错误页、几何裁剪 |
| 单实例 | `src-tauri/src/single_instance.rs` | `$XDG_RUNTIME_DIR/dsh-tauri.sock` |

## 验证命令

```bash
npm run check   # 检查全部 tools 脚本语法
npm run check:docs                           # 校验三处文档测试数与 Rust 源码一致（verify:fast 已包含）
npm run check:versions                       # 三处版本号一致性检查（verify:fast 已包含）
npm run check:locales                       # 校验七语言 locale JSON、meta 三字段与 detail 文档路径（verify:fast 已包含）
npm test                                     # 拉起 dsh web，验证 303 + cookie + 客户端 HTML
npm run test:unit                            # 注入脚本契约单测（node --test test/*.test.mjs，无需运行时）
npm run test:contract                        # 上游桥接契约漂移守卫（只读扫描 runtime/，缺失时 SKIP）
npm run verify:fast      # 离线快速门禁：语法检查 + 文档测试数校验 + npm 单测 + 桥接契约守卫
npm run verify:all      # 全量离线门禁：verify:fast + Rust 单测
npm run verify:e2e   # 全量离线门禁 + 真运行时探针（自动使用工作区临时 DSH_HOME 并清理）
npm run cargo -- test                        # Rust 单测
npm run cargo -- build                       # Rust 构建
npm run build:check   # 链接真正的生产二进制（cfg(test) 之外的代码）
```

`npm run cargo` 是 cargo 的薄包装脚本：默认把 `CARGO_TARGET_DIR` 指向工作区内的
`src-tauri/target`，避开全局 `~/.cargo/config.toml` 里只读 target-dir 导致的失败；
若调用方已显式设置 `CARGO_TARGET_DIR`，则尊重该设置。

三项 npm 测试的分工：`npm test` 是**真运行时契约探针**（拉起真实 `dsh web` 验证 303 +
cookie + 客户端 HTML）；`npm run test:unit` 测的是 **SHIM 注入脚本自身的行为**（纯 node 单测，
不需要运行时）；`npm run test:contract` 是**上游升级后的契约漂移守卫**（只读扫描 `runtime/`，
上游桥接缺失时 SKIP）。排查时先跑 `test:unit` 定位脚本自身问题，再用 `npm test` 端到端确认；
CI 亦执行这三项。

另有一条 `npm run verify:fast` 可一次跑完上述检查中的 `check` + `check:docs` + `check:versions`
+ `check:locales` + `test:unit` + `test:contract` 六层：`check:docs` 校验三处文档（README.md、CHANGELOG.md、
`docs/DESKTOP_BRIDGE.zh.md`）里硬编码的 Rust 单测数与 `src-tauri/src/**/*.rs` 的实际
`#[test]` 数量一致，漂移即 FAIL；`check:docs` 还校验 `tools/check-bridge-contract.mjs`
里 `GUARD_CHECK_COUNT` 声明的契约守卫检查项数与 CHANGELOG.md 中「契约漂移守卫…静态检查」
的项数一致（只读源码正则比对，不运行守卫），漂移即 FAIL；`check:versions` 以 package.json 为基准核对
`src-tauri/tauri.conf.json` 与 `src-tauri/Cargo.toml` `[package]` 的版本号，漂移即 FAIL，
已包含在 `verify:fast` 内；`check:locales` 只读扫描 `locale/*.json`，校验七语言（zh/en/es/fr/de/ru/pt）集合恰好齐全、每个 JSON 合法且 `meta.title` / `meta.description` / `meta.detail` 为非空字符串、`meta.detail` 指向的文档存在，漂移即 FAIL；全程离线、不需要真运行时，runtime 缺失时
`test:contract` 会自动 SKIP；但真正的
端到端确认仍须跑 `npm test`（需要运行时）。`npm run verify:all` 则在 `verify:fast`
基础上再跑一遍 `cargo test`（经 `tools/cargo.mjs` 包装脚本调用，无需手动设置
`CARGO_TARGET_DIR`）；`verify:all` 已包含 `build:check` 链接检查（在测试之前，
链接真正的生产二进制，覆盖 `cfg(test)` 之外的代码）；同样地，端到端真探针仍需 `npm test`。
`npm run verify:e2e` 则在 `verify:all` 之后再跑一遍真运行时探针（`test:clean` 包装脚本）：
每次在运行中自动建工作区内临时目录 `.dsh-probe-<pid>` 作为 `DSH_HOME`，结束时
自动删除，不碰 `~/.dsh`，失败或中断也不会留下残留。

桌面桥接的精确契约，以及「`dshDesktop` 明确不实现」的决策与理由，见
[`docs/DESKTOP_BRIDGE.zh.md`](DESKTOP_BRIDGE.zh.md)。

`npm test` 需要先装运行时：

```bash
npm install --prefix runtime @deepseek-ai/dsh
```

探针需要一个**可写**的 `DSH_HOME`（默认 `~/.dsh`）；若该目录只读，用
`DSH_HOME=/path/to/writable npm test` 覆盖；CI 已显式设为 runner 临时目录。

## 关键实现注意点

1. **服务 URL 只从 stdout 拿**：`dsh web` 打印 `dsh web: http://127.0.0.1:<port>/?token=…`；
   不要自己猜端口，`--port 0` 让系统分配。
2. **窗口必须在拿到 URL 之后直接创建在该 URL 上**。先建 splash 窗口再 `navigate` 会让
   WebKit 丢掉认证重定向的 `Set-Cookie`（表现为页面只剩
   `dsh web authentication required`）。所以用独立 label 的 splash 窗口，命中 URL 时
   另建 `main` 窗口再销毁 splash。
3. **注入脚本要在页面脚本之前**：`WebviewWindowBuilder::initialization_script`。
   它依赖 `withGlobalTauri`（见 `tauri.conf.json`）提供的 `window.__TAURI__.core.invoke`。
4. **查询字符串里的 token 是一次性认证入口**：`GET` 返回 303 并下发 `dsh-auth-<authority>`
   cookie（`HttpOnly; SameSite=Strict`）。服务端按 authority 生成 cookie 名，端口变了名字就变。
5. **退出走 SIGTERM**：`dsh::shutdown()` 先 `kill -TERM`，5 秒后仍未退出才 SIGKILL，
   避免会话库留下 WAL 残渣。
6. **托盘退出**要先 `dsh::shutdown()` 再 `app.exit(0)`，否则会留下孤儿 node 进程。

## 运行时查找顺序

`DSH_TAURI_RUNTIME` → 仓库内 `runtime/`（存在 `node_modules` 才算）→
`~/.dsh/dsh-tauri/runtime` → 打包资源目录下的 `runtime/`。

都找不到时显示错误页，正文里带安装命令。

`runtime::Source` 记录命中哪一档，只有 `Home` 和 `Resource` 是外壳自己的目录，
自动更新可以写入；`Explicit`（`DSH_TAURI_RUNTIME`）和 `Checkout`（仓库内）
只打日志，绝不动——那里可能是 git 跟踪的文件。

## 首次启动自动安装

`runtime::ensure` 在四档都找不到运行时、且不是离线模式时调用
`install::install(dir, "latest")`，装到 `~/.dsh/dsh-tauri/runtime`：

```
runtime::ensure          定位失败 → 非离线 → install_fresh
  install::npm_available 先跑 `npm --version`，缺 npm 给可读错误
  swap::Staging::new     建 <runtime>.new-<pid> 暂存目录（先清掉旧的临时树）
  exec::run              npm install --prefix <staging> --no-audit --no-fund
                         --loglevel=error @deepseek-ai/dsh@latest（600 秒超时）
  has_package            校验 node_modules/@deepseek-ai/dsh/lib/bin.js 存在
  Staging::commit        rename 换上去，旧的改名 .old-<pid>
```

- 安装期间 `progress::set(Stage::Installing, …)`，splash 轮询 `runtime_status`
  显示阶段与已用时；
- `exec.rs` 让 npm 跑在自己的会话里，超时或退出时按进程组整组收，
  不留 postinstall 孙进程；
- 任何一步失败都返回可读错误 + 手动命令，走 `progress::fail` → 错误页。

## 自动更新

服务启动后 `update::spawn_once(dir, source)` 起一条 `dsh-update` 线程（进程内只跑一次）：

1. `version_at(dir)` 读本地 `node_modules/@deepseek-ai/dsh/package.json` 的 version；
2. `npm view @deepseek-ai/dsh version` 拿上游版本（90 秒超时，遵循用户的 npm 镜像配置）；
3. `version::is_newer` 比较（纯 Rust 语义化比较，含 prerelease 规则，见 `version.rs` 单测）；
4. 更新则 `progress::set(Stage::Updating, …)` → `install::install(dir, "latest")`
   走同一套暂存 + 原子替换 → `notify-send -a DSH "DSH 已更新" "已更新到 x.y.z，重启生效"`。

约束：不杀正在跑的服务（新版本下次启动才加载）、失败只打
`[runtime] 更新检查失败（下次启动再试）` 并在下次启动重试、`Source::updatable()`
为假时只记一行日志。

## 原子替换与清理

- 暂存目录 `<runtime>.new-<pid>`、旧树 `<runtime>.old-<pid>` 都是运行时的**同级兄弟目录**，
  保证 `rename` 同文件系统、原子；
- `commit` 先把旧目录 rename 成 `.old-<pid>`，再把暂存 rename 成正式目录；
  第二步失败会把旧目录 rename 回来（错误信息里写明「旧运行时已恢复」）；
- `Staging::drop` 在没 commit 时删掉暂存树 —— 安装中断不会留半成品；
- 旧树**留给仍在跑的服务**（它的模块已从旧树加载），因此更新后磁盘上会同时存在两份
  （约 1.2G）；`swap::clean_stale_runtime()` 在 `main.rs` 的 `.setup` 里、
  单实例抢到之后调用，清掉同级目录中所有 `<name>.new-*` / `<name>.old-*`，
  失败只打日志。

## 两个开关

| 变量 | 取值 | 效果 |
| --- | --- | --- |
| `DSH_TAURI_OFFLINE` | `1/true/yes/on` | 关掉安装与检查（全部联网行为）；无运行时则错误页 |
| `DSH_TAURI_AUTO_UPDATE` | `0/false/no/off` | 只关版本检查，首次启动的自动安装照常 |

实现见 `runtime.rs::offline()` / `auto_update_enabled()`；`auto_update_enabled`
先把 `OFFLINE` 短路掉，所以离线时两者都关。

## 调试自动安装与更新

**假 npm**（不联网、不下载 1.2G，验证整条安装/更新链路）：`install::npm()` 认
`DSH_TAURI_NPM`，写一个可执行脚本冒充 npm 即可。

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

- **首次启动安装**：把 `~/.dsh/dsh-tauri/runtime` 挪走（`mv` 到 `/tmp`），
  `DSH_TAURI_NPM=/tmp/fake-npm cargo run`，应看到 splash 进入「正在安装」阶段，
  结束后 `~/.dsh/dsh-tauri/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js` 存在；
  但假包没有真服务，窗口最终会停在启动失败——验证安装链路足够。
- **更新路径**：装好后让 `FAKE_LOCAL=1.0.0`、`FAKE_UPSTREAM=2.0.0` 再启动，
  日志应出现 `发现新版 2.0.0（本地 1.0.0）`、`已原子替换`、`旧运行时保留至下次启动清理`，
  同级目录出现 `<runtime>.old-<pid>`；再启动一次应打印 `清理上次运行留下的 …`。
- **开关**：`DSH_TAURI_OFFLINE=1` 且无运行时 → 错误页正文带 `npm install --prefix`
  命令；`DSH_TAURI_AUTO_UPDATE=0` → 日志出现 `自动更新已关闭`，但首次安装仍会执行。
- **安装失败**：让假 npm 的 `install` 分支 `exit 1` → 错误页带 npm 输出与手动命令，
  且原运行时不受影响（`<runtime>.new-<pid>` 已被 `Drop` 清掉）。
- **收尾**：`pkill -x dsh-tauri`；确认 `~/.dsh/dsh-tauri/` 下没有 `.new-*` / `.old-*`
  残留，假 npm 与挪走的 runtime 记得删掉/放回。

## 冒烟检查（人工）

```bash
cd src-tauri && cargo run          # 应看到 splash，随后出现完整 UI
curl -s -o /dev/null -w '%{http_code}\n' "$(grep -ao 'http://127.0.0.1:[0-9]*/?token=[A-Za-z0-9_-]*' /tmp/xxx.log | head -1)"
```

窗口内应能看到：会话列表、工作区、模型选择器、正在运行的会话流。
