# 更新记录

## 未发布（Unreleased）

**安全加固**

- **启动 URL 门禁**：启动地址只接受 loopback origin，拒绝外部站点传入的地址，防止外部站点劫持窗口加载
- **外链校验**：把外链 URL 校验提取为纯函数，拒绝控制字符，错误消息不再回显完整 URL
- **DSH_TAURI_HOST 校验**：启动前校验该变量，避免与 loopback 门禁冲突时静默失败
- **目录选择取消**：改用退出码判定用户取消，不再依赖匹配错误文本
- **通知参数注入防护**：notify 传参加选项终止符，防止标题或正文以 `-` 开头时被当成选项解析

**运行时健壮性**

- **助手进程登记表**：支持并发登记，并用 RAII 守卫保证所有退出路径都注销 PID，避免 SIGKILL 后复用 PID 被误杀
- **单实例 socket 抢占**：改为有界循环并只删除真正的 socket 文件，防止递归栈溢出与误删
- **窗口状态原子写入**：先写临时文件再替换，避免并发写坏状态文件
- **陈旧运行时清理**：只精确匹配 `<name>.new-<pid>` / `.old-<pid>`，不误删用户目录
- **上游版本号解析**：加语义化校验，避免把 npm 错误文本当成版本号而误判为有更新
- **shell_info**：改为只读定位，避免 `info()` 触发并发 npm 安装
- **优雅退出**：改用 `libc::kill` 发送信号并检测失败，避免信号发送失败后仍误判退出成功
- **窗口创建失败兜底**：主窗口创建失败时显示可读错误页，而不是静默退出
- **错误页 hash 容错**：对畸形 hash 做容错解析，避免异常片段导致错误页渲染失败
- **启动页轮询**：轮询加序号防乱序，并在进入终态后停止，避免旧响应覆盖最新状态

**测试与质量**

- **测试整理**：删除失效的 shell-api 测试，新增 SHIM 契约单测（21 项断言）与 probe-cli 失败路径测试
- **契约漂移守卫**：新增上游桥接契约静态检查（11 项（含 SHIM 双窗口注入点））并接入 CI
- **探针健壮性**：补上探针超时与信号清理
- **Rust 单测扩容**：从 17 个增至 107 个，覆盖 swap 原子替换、runtime 查找、banner loopback、native 外链与取消、通知参数、剪贴板退出状态、exec 并发与 RAII、single_instance、DSH host 校验、update 版本解析与 shell_ui 原子写、supervisor 优雅退出、install npm 安装参数
- **错误页与启动页单测**：新增 node:vm 单测（错误页 6 项、启动页 10 项），npm 单测总数达 23 项

**工程**

- **CI 增强**：接入单测与守卫，并加上 job 与步骤超时
- **桌面桥接文档**：新增 `docs/DESKTOP_BRIDGE.zh.md`，记录桌面桥接契约与「dshDesktop 明确不实现」的决策
- **开发手册**：同步新的验证命令
- **打包元数据**：`longDescription` 更新为当前架构说明
- **离线验证门禁**：新增 `verify:fast` 一键离线验证命令，并补全脚本语法检查
- **Node 24 对齐**：CI 与 `engines` 统一对齐 Node 24

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
