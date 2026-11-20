# 开发与验证

## 分层

| 层 | 位置 | 说明 |
| --- | --- | --- |
| 外壳 | `src-tauri/src/` | Tauri 应用：安装探测、子进程监督、窗口/菜单/托盘、窗口状态 |
| 桥 | `bridge/` | Node 侧：fork 官方 Host、回环反代、原生 API、renderer shim |
| 探针 | `tools/` | 契约验证与 asar 提取，不参与打包 |
| 文档 | `docs/CONTRACT.md` | 从官方外壳逆向出来的契约，改动前先读 |

## 验证命令

```bash
npm run check                  # 三个 bridge 脚本的 node --check
npm test                       # shell API 路由测试（不需要真机 Host）
node tools/probe-host.mjs      # 直连 Host：认证、反代头规则、index 注入
node tools/probe-bridge.mjs    # 完整链路：bridge + 反代 + WebSocket 升级
cd src-tauri && cargo build    # 外壳编译
npm run build                  # 产出 .deb 与 AppImage
```

`probe-bridge` 期望看到：`ready`、认证 303 + set-cookie、`GET /` 200 且含 `#root`、
`/api/remote.mux` 经代理返回 101。

## 冒烟钩子

没有可用的合成输入通道（Wayland 下 `wtype` 拿不到 virtual-keyboard 协议，`ydotool` 需要
root），所以窗口关闭路径由一个环境变量驱动：

```bash
# closeToTray 为真：窗口隐藏，进程与 Host 继续存活，不写窗口状态
DSH_TAURI_SMOKE_CLOSE=1 ./src-tauri/target/debug/dsh-tauri

# closeToTray 为假：进程退出，并写入 ~/.dsh/dsh-tauri-window-state.json
# （改 ~/.dsh/desktop-preferences.json 的 closeToTray 后重跑同一条命令）
```

窗口尺寸在下次启动时恢复；位置只在平台允许由客户端摆放时才生效。

## 调试安装探测

```bash
DSH_TAURI_APP_ROOT="/opt/DSH NEXT/resources/app.asar" cargo run
RUST_LOG=debug cargo run          # Tauri/GTK 侧日志
```

`lib/host.js` 的 stdout 会被 bridge 转成 `log` 行打到自己的 stderr，stderr 尾部保留 16KB，
Host 退出时会随 `exit` 事件一起上报。

## 发布

`npm run build` 之后产物在 `src-tauri/target/release/bundle/{deb,appimage}/`。
CI（`.github/workflows/build.yml`）在每次 push 上跑同样的命令并把两个包作为 artifact 上传。

## 单实例与崩溃恢复

- `src-tauri/src/single_instance.rs`：在 `$XDG_RUNTIME_DIR/dsh-tauri.sock` 上监听。第二次启动
  连上该 socket 发 `focus` 后 `exit(0)`；主实例收到后显示/取消最小化/聚焦主窗。socket 随进程
  消失，残留文件在下次启动时被重新接管。
- `bridge.rs` 的 `run()` 在 Host 意外退出时最多重启 3 次（间隔 2s），每次重新 spawn bridge；
  超限则弹错误窗口（`shell_ui::build_error_window`）。托盘「退出」走 `bridge::shutdown()`，
  置 `SHUTTING_DOWN` 后不再重启。
- 注意：Host 被 `SIGKILL` 后立刻重启可能因 profile 里的残留状态启动失败（日志里是
  `CordisError: cannot create effect on inactive context` + `authentication failed (status 404)`），
  此时再启动一次应用即可恢复。

## 通知

`bridge.rs` 把 Host 的 `notification` 事件交给 `shell_ui::show_notification`：

- 先看 `~/.dsh/desktop-preferences.json` 的 `notifications` 与按结果分的开关
  （`turnCompleted` / `turnFailed` / `scheduleCompleted` / `scheduleFailed`），默认开；
- 主窗有焦点时不打扰；
- 文案走 `shell_ui::notification_text`（回合完成用消息本身，其它用固定句子，中英按 `LANG`），
  再交给 `notify-send` 发系统通知。
