# 更新记录

## 0.4.1

- **签名发布**：补齐打包产物的签名配置与发布流程，deb / AppImage 可经验签后分发
- **Wiki 六页**：新增六页项目 Wiki，覆盖安装、使用、开发与打包等主题
- **安全配置**：加入 Dependabot 依赖更新、CODEOWNERS 代码归属与 SECURITY 漏洞披露政策，并启用 GitHub 安全功能
- **Git Flow 规范**：补充分支命名与合流规范（feature / release / hotfix）及提交约定
- **七语言本地化**：界面文案扩展为七种语言，简体中文优先，不含日语
- **star 提示**：加入仓库 star 引导提示，方便用户支持项目

## 0.4.0

- **首次启动自动安装**：缺运行时不再让人手动 `npm install`，外壳自己把
  `@deepseek-ai/dsh` 装到 `~/.dsh/dsh-tauri/runtime`，装完直接起服务
- **启动后自动更新**：服务起来后在后台跑 `npm view` 比对版本，有新版自动安装并用
  `notify-send` 提示「重启生效」；正在跑的服务不会被杀，新版本下次启动才加载
- **原子替换**：安装写在同目录的 `<runtime>.new-<pid>` 暂存目录，成功后才 `rename`
  换上去；失败或中断时旧运行时原样保留，不会出现半装的 `node_modules`
- **启动进度**：splash 显示当前阶段（查找 / 安装 / 启动 / 更新）与已用时，出问题时显示
  可读错误页与手动修复命令
- **两个开关**：`DSH_TAURI_AUTO_UPDATE=0` 只关版本检查、`DSH_TAURI_OFFLINE=1` 同时关掉
  安装与检查（离线也能启动已有运行时）
- **旧运行时保留到下次启动清理**：更新后旧树改名为 `<runtime>.old-<pid>` 供仍在跑的
  服务继续读取，下次启动时统一清掉 `.new-*` / `.old-*`
- 新增模块 `runtime` / `install` / `update` / `swap` / `progress` / `exec` / `version` /
  `supervisor` / `banner`，`shell_ui` 增加 splash 与错误页

## 0.3.0

- **换本体**：不再驱动 `/opt/DSH NEXT` 的 Electron 安装，改为拉起 npm 上的
  `@deepseek-ai/dsh`（`dsh web`，纯 Node 服务）；窗口直接加载它打印的
  `http://127.0.0.1:<port>/?token=…`
- 删掉整条逆向出来的桥接层：回环反向代理、渲染令牌、`dsh-app://` 协议、fork IPC 转发、
  preload 兼容层都不再需要
- 原生命令换成 Tauri 命令 + 注入脚本：目录选择、剪贴板、系统通知、外链、标题同步
- 服务生命周期：从 stdout 解析 URL、崩溃自动重启（最多 3 次）、`PR_SET_PDEATHSIG`
  保证外壳退出时带走服务进程
- 打包不再内置 `node_modules`：运行时按 `DSH_TAURI_RUNTIME` → 仓库 `runtime/` →
  `~/.dsh/dsh-tauri/runtime` 查找，缺失时显示带修复命令的错误页
- `tools/probe-web.mjs`：一条命令验证运行时契约（303 + 会话 cookie + 客户端 HTML）

## 0.2.1

- 修复打包：`bridge/asar.mjs` 没有随包发布，导致安装版启动时报 `ERR_MODULE_NOT_FOUND`；
  resources 改为按 `bridge/*.mjs`、`bridge/*.js` 收集，新增文件不会再漏

## 0.2.0

- 识别上游版本：启动日志打印 `[dsh-tauri] DSH Desktop <version>`，认证失败时错误消息带版本
- 单实例守卫：第二次启动聚焦已有窗口后退出，不会在同一 profile 上起第二个 Host
- Host 崩溃自动重启（最多 3 次），超限弹可读的错误窗口
- 平台登录：独立登录窗 + `/oauth/callback` 经代理以原生渲染器身份重放
- 系统通知：回合 / 自动化任务完成或失败，按偏好与窗口焦点过滤，文案与官方一致
- 窗口标题跟随 `document.title`；窗口尺寸与最大化跨启动保留
- 拖拽文件回传绝对路径（`__DSH_HOST_PATHS__`）、目录选择、剪贴板、外链、权限探测
- 打 tag 时自动把 `.deb` 与 AppImage 附到 Release

## 0.1.0

- 首个可用版本：Tauri 2 外壳驱动官方 DSH Host，桌面模式、实时流、插件与 profile 全部沿用
- 回环反向代理：补会话 cookie 与渲染令牌，转发 HTTP 与 WebSocket 升级
- 原生菜单栏、托盘与关闭到托盘；deb / AppImage 打包与 CI
