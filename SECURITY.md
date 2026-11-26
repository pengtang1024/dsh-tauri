# 安全策略

## 支持版本

| 版本 | 是否接收安全修复 |
| --- | --- |
| `0.4.x`（当前维护线，从 `0.4.0` 起） | 是 |
| `0.3.x` 及更早 | 否，请先升级到受支持的最新版本 |

当前只维护 `0.4.x` 版本线。旧版本的问题如果在新版仍然存在，会在新版修复；不单独为旧版本发补丁。

## 私密报告

**不要为安全漏洞开公开 issue、PR 或 discussion**，也不要在公开渠道贴出可利用细节。请任选一种私密方式：

1. GitHub Security Advisories（首选）：<https://github.com/pengtang1024/dsh-tauri/security/advisories/new>
2. 邮件：<pengtang1024@qq.com>，主题建议加 `[SECURITY]` 前缀。

报告里尽量写清：

- 受影响的 `0.4.x` 具体版本，以及操作系统 / WebKitGTK 版本；
- 复现步骤或最小 PoC；
- 实际影响（例如能否读到会话令牌、执行任意命令、越权打开本地文件或破坏运行时目录）；
- 如果方便，附上修复建议。

## 响应时限

- **3 个工作日内**确认收到报告，并给出初步判断。
- 确认是有效问题后，会说明修复计划；修复发布后，在 GitHub Security Advisory 中记录影响版本与修复版本。
- 如果判断不属于本仓库范围，会说明原因并指向对应的上游渠道。

## 范围

本仓库负责**桌面外壳本身**的安全问题：

- `src-tauri/src/dsh.rs` 中 `dsh web` 服务的定位、spawn、URL 解析与崩溃重启；
- `src-tauri/src/runtime.rs`、`install.rs`、`update.rs`、`swap.rs` 的运行时定位、安装、版本比对与原子替换；
- `src-tauri/src/native.rs` 中的原生命令与注入脚本（`SHIM`），包括剪贴板、系统通知、目录选择与外链打开；
- `src-tauri/src/shell_ui.rs`、`single_instance.rs`、`exec.rs`、`banner.rs`、`progress.rs`、`version.rs`、`supervisor.rs` 的窗口 / 托盘、单实例守卫、子进程收尾与启动日志解析；
- `tools/probe-web.mjs` 运行时契约探针，以及 `.github/workflows/` 的构建与发布流程。

**不属于本仓库范围**：`@deepseek-ai/dsh` 官方 npm 包本体（含 Web UI、插件、provider 路由与凭据存储）、Tauri / WebKitGTK / wlr-clipboard 等上游组件、系统包管理器与发行版配置、模型服务与平台账号。这类问题请走对应上游渠道：

- <https://github.com/deepseek-ai/deepseek-harness/security>
- 或相应上游仓库 / 官网公布的其它安全报告入口。

本项目根 `package.json` 只声明开发依赖 `@tauri-apps/cli`，`runtime/package.json` 是本地开发用的安装目录；Dependabot 会按周检查两处 npm 依赖与 GitHub Actions 更新。发布包不随包分发 `node_modules`：运行时由外壳首次启动安装到用户目录，之后由用户自己的凭据向 npm 拉取。

## 安全设计说明

- **会话令牌只走本地回环 URL。** 外壳从 `dsh web` 的 stdout 里解析出 `http://127.0.0.1:<port>/?token=…` 并直接指向窗口，令牌不写入仓库、日志或配置文件；日志只记录解析失败与阶段信息，不打印完整 URL。
- **外链打开有白名单。** `native.rs` 的 `open_external` 只接受 `http(s)` 与 `mailto`，拒绝其它 scheme，避免渲染进程里的远程内容借 `xdg-open` 触发本地程序；目录选择、剪贴板等原生能力只通过显式的 Tauri 命令暴露。
- **子进程不用字符串拼命令。** npm、node、zenity、wl-copy 等都以 `Command::new(program)` 加独立参数启动，不经过 shell 字符串拼接；辅助进程带超时并在外壳退出时按进程组一起收尾（含 `PR_SET_PDEATHSIG`），避免残留。
- **运行时更新是原子的。** 安装先写 `<runtime>.new-<pid>` 暂存目录，成功后才 rename 替换；失败或中断不破坏已装好的运行时。`.old-<pid>` 供仍在跑的服务读取，下次启动统一清理。
- **网络行为可关。** `DSH_TAURI_OFFLINE=1` 关掉安装与版本检查，`DSH_TAURI_AUTO_UPDATE=0` 只关版本检查；`DSH_TAURI_RUNTIME` 指定的目录与仓库内 `runtime/` 属于用户或 git 管理，外壳只记日志不写入。
- **单实例防重复服务。** 第二次启动通过 Unix socket 聚焦已有窗口后退出，不会在同一 profile 上起第二个 Host 服务。

## 已知边界

- 回环 URL 里的 token 是访问 Web UI 的凭据，虽然只在本机监听，仍不要把含 `token=` 的完整 URL 贴进 issue、日志或对话。
- 运行时目录里的 `node_modules` 是明文安装的 npm 包；请从 npm 官方源安装，不要替换成来源不明的包，`DSH_TAURI_RUNTIME` 指向的目录请自行保证可信。
- 外壳会校验 `DSH_TAURI_URL` 只允许 loopback http(s) 地址，但仍不校验该服务返回的内容：开发时用该变量直连已在运行的服务，只应指向你信任的本地实例。
- 自动更新会让下次启动加载新版本本体；企业或受限环境如不接受该行为，请显式设置 `DSH_TAURI_OFFLINE=1` 或 `DSH_TAURI_AUTO_UPDATE=0`。
- 本项目目前按 Linux / Wayland + WebKitGTK 编写（zenity、wl-clipboard、notify-send），其它平台的原生命令行为未做安全审计。
