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
node --check tools/probe-web.mjs
npm test                                     # 拉起 dsh web，验证 303 + cookie + 客户端 HTML
cd src-tauri && cargo test && cargo build    # Rust 单测 + 构建
```

`npm test` 需要先装运行时：

```bash
npm install --prefix runtime @deepseek-ai/dsh
```

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

## 冒烟检查（人工）

```bash
cd src-tauri && cargo run          # 应看到 splash，随后出现完整 UI
curl -s -o /dev/null -w '%{http_code}\n' "$(grep -ao 'http://127.0.0.1:[0-9]*/?token=[A-Za-z0-9_-]*' /tmp/xxx.log | head -1)"
```

窗口内应能看到：会话列表、工作区、模型选择器、正在运行的会话流。
