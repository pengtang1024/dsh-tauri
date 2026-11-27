# 更新记录

## [未发布]

- **窗口几何记忆路径文档与守卫**：README 与七语言详情页写明窗口状态文件 `~/.dsh/dsh-tauri-window-state.json` 的位置与作用（记录尺寸、位置与最大化状态，下次启动据此恢复，不写死 uid、不随 `DSH_HOME` 改变）；文档守卫只读校验 `shell_ui.rs::state_path()` 中 `".dsh"` 与 `"dsh-tauri-window-state.json"` 两个字面量未变，并核对八处文档路径一致，防止几何记忆静默失效。
- **调查记录（未修复）**：记录 Linux/Wayland 下窗口放大后 L 形黑边的实机结论——现象为右侧/底部纯黑、WebView 绘制面停在旧尺寸；根因是 WebKitGTK 2.52.6 + Wayland + `GtkBox` 下 `set_bounds` 不触发绘制面高度重分配且冷启动不触发 `Resized`；已证伪 `set_size` 自激（崩溃）与 `queue_resize`（只修横向、纵向恒定）两个方案；后续探索导航完成后有界 `size_allocate` 或等待上游修复。方案 A 的未提交代码已还原，此问题仍为已知问题、未修复（详见 `docs/DEVELOPMENT.md`）。

## 0.6.0

- **deb 运行时助手依赖**：把 README 声明的四个运行时助手（`zenity` / `wl-clipboard` / `libnotify-bin` / `xdg-utils`）补进 `tauri.conf.json` 的 `bundle.linux.deb.depends`，Debian / Ubuntu 的 `.deb` 安装后目录选择、剪贴板、通知与外链开箱即用（Tauri 的 depends 是覆盖语义，需显式声明）；README 与七语言详情页的依赖段同步该口径并在 apt 示例补上 `xdg-utils`。
- **菜单按项目工作流重新设计**：放弃通用桌面模板裁剪，窗口菜单收敛为单栏 `DSH`（重新加载 / 开发者工具 / 关于簇），每项只对应本项目工作流——External URL 无浏览器刷新键所以需要重新加载、WebKitGTK 上排查 Web UI/桥接需要开发者工具、外壳独有的 shell_info 需要关于面板回答「跑的是哪个运行时」；「关于」簇内含「关于 dsh-tauri」与「支持项目 ⭐」，不设编辑/查看/窗口栏与全屏（上游自管剪贴板、GTK F11 与标题栏已覆盖），托盘保持显示主窗口/退出不动。
- **开发者工具 release 可用**：菜单项始终显示、始终可打开，无禁用态、无环境变量门；Cargo.toml 的 `devtools` feature 保持不变，不再出现「看得见点不动」。
- **关于面板**：Tauri 2.12.7 本体无 dialog API（可选插件未引入、0 浸入不新增依赖），用系统 `zenity --info --no-markup` 展示 shell_info（运行时/主目录/配置/桌面版本），空值回退「未设置」。
- **运行状态面板**：把「关于 dsh-tauri」扩成桌面端运行状态/诊断面板，体现这个可插拔 npm 运行时外壳独有的特性——运行时来源档位（Explicit/Checkout/Home/Resource）与版本（回答「跑的是哪一档、更新是否生效」）、自动更新状态（关闭 / 开启但不自动写入 / 开启且下次生效，只读判定、不触发检查）、`closeToTray` 关窗语义（托盘保活 vs 退出，保活一个 Node Host + 会话）、五个原生助手（zenity / wl-copy / wl-paste / notify-send / xdg-open）的只读 PATH 可用性、SHIM 协议版本 1 与「不实现 dshDesktop / 不设置 data-platform」的桥接事实；沿用 `zenity --info --no-markup` + `.spawn()` 非阻塞，零新依赖、零浸入 upstream，菜单仍单栏 DSH、托盘不变。
- **重新加载修复**：改用 `WebviewWindow::reload()`（原生 WebView 重载）替代在 External URL WebView 上不可靠的 `eval("window.location.reload()")`，失败时输出带上下文的日志。
- **Rust 单测**：重写菜单结构静态守卫（单栏工作流、关于簇、devtools 门控、reload 方式、关于面板格式与 zenity 安全参数），总数达 183 项（含崩溃计数稳定窗口 5 项、建窗失败错误页守卫 2 项）。
- **崩溃重启计数修复**：就绪后稳定 60 秒再崩溃时归零重启计数，避免相隔数小时的偶发崩溃累计触发错误页；短时连续崩溃仍保留 3 次风暴保护（纯函数覆盖门槛边界与连续崩溃）。
- **建窗失败错误页**：`show_url` 在主线程建窗或导航失败时直接结束 boot 进度并展示可读错误页，替换卡住的 splash；不再只写日志后让 supervisor 阻塞在静默等待。
- **docs**：核心目标统一为三档口径——Linux 深度支持、第一优先（优先支持各种发行版与不同版本，力求最佳兼容性、性能与最低资源消耗），是唯一完成适配与验证的平台；Windows / macOS 基础支持，能构建能运行，但不做深入适配与优化、未做全面验证；移动端（iOS / Android）不支持，原因是架构上依赖本地 Node 服务 + 系统 WebView，移动端沙箱与后台限制不适用；Windows / macOS 用户建议直接从官方渠道下载官方桌面版本，本项目在 Win/Mac 上仅提供基础支持、不作为首选方案。

## 0.5.0

**新增功能**

- **star 入口**：菜单与托盘加入支持项目 star 入口，方便用户支持项目

## 0.4.2

**安全加固**

- **启动 URL 门禁**：启动地址只接受 loopback origin，拒绝外部站点传入的地址，防止外部站点劫持窗口加载
- **外链校验**：把外链 URL 校验提取为纯函数，拒绝控制字符，错误消息不再回显完整 URL
- **DSH_TAURI_HOST 校验**：启动前校验该变量，避免与 loopback 门禁冲突时静默失败
- **目录选择取消**：改用退出码判定用户取消，不再依赖匹配错误文本
- **通知参数注入防护**：notify 传参加选项终止符，防止标题或正文以 `-` 开头时被当成选项解析
- **probe 参数校验**：CLI 参数缺失或非法时给出清晰错误并直接退出，不再打印堆栈
- **环境变量开关大小写不敏感**：离线等开关统一忽略大小写与首尾空白，避免写成 `TRUE` / `Off` 时离线意图失效
- **probe banner URL 解析**：探针解析 banner URL 与外壳对齐，只接受 http/https + loopback（127.0.0.1 / localhost / [::1]），匹配大小写不敏感，并用 URL 类复核挡住畸形地址
- **DSH_TAURI_URL 开发钩子门禁**：开发用的 URL 覆盖入口也要求 loopback http(s) 地址，与 banner 解析共用 `is_safe_app_url` 判定，外部地址直接忽略并给出提示
- **安全文档过期表述纠正**：SECURITY 中英文文档改掉「外壳不校验 `DSH_TAURI_URL`」的旧说法，如实写明现只允许 loopback http(s) 地址，但仍不校验该服务返回的内容
- **安全文档镜像源澄清**：SECURITY 中英文文档说明开发期 lockfile 走 npm 镜像加速、每条依赖均带 sha512 完整性校验，生产自动安装仍走 npm 默认源，与「从官方源安装」的建议并不矛盾
- **认证重定向门禁**：真探针校验认证 303 的 Location 解析后必须保持本地 origin，相对路径与同源绝对 URL 合法，缺失或跨 origin 即 FAIL；失败文案不回显完整目标以防 token 泄露
- **启动静默卡死保护**：dsh web 启动后存活但不打印 URL 时，外壳原会永久停在 splash；现增加 60 秒 watchdog（stdout 独立线程 + mpsc），超时经子进程句柄回收并按关停状态分流，进入可读错误页且不重试；URL 命中后不受超时约束，避免慢机器误伤

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
- **启动页轮询**：用 inFlight 标志避免并发请求与响应乱序，进入终态后停止轮询，避免旧响应覆盖最新状态
- **swap 回滚失败可见**：原子替换回滚失败时如实报错，不再谎报成功
- **首次安装失败文案纠正**：首次安装（原本无旧运行时）stage → target 重命名失败时，不再提示恢复不存在的 `<old>` 路径或给出必然失败的 `mv` 命令，改为说明未改动既有运行时、暂存树仍在原处并提示检查权限/磁盘后重试；既有运行时的两条分支文案不变
- **npm 版本探测诊断**：版本探测失败时输出可诊断信息，便于定位根因
- **版本解析收敛**：取输出中的最后一个版本号并抑制 npm notice，避免噪声干扰解析
- **单实例 socket 隔离**：socket 路径按用户隔离，避免多用户之间互相抢占
- **托盘退出保存窗口**：从托盘退出时保存窗口几何，下次启动恢复到位
- **托盘启动 splash**：启动阶段托盘显示 splash，反馈初始化进度
- **SIGKILL 竞态收敛**：超时宽限期内已回收的进程组不再重复发送 SIGKILL
- **运行时定位统一**：只读定位收敛为单一实现，消除双实现带来的行为漂移
- **退出幂等化**：shutdown 增加一次性门闩，托盘退出与 Exit 事件重复触发时只拆一次服务，避免对已回收进程重复发信号
- **kill_running 竞态收敛**：宽限期后只强杀仍登记的进程组，已退出并注销的 PID 不再被误杀
- **助手输出非 UTF-8 容错**：按字节读取并做无损替换，无效字节不再吞掉整段诊断输出
- **助手输出采集有界化**：drain 改用 mpsc + recv_timeout（200ms）替代无条件 join，孙进程继承管道不再挂住外壳，超时记日志并继续
- **窗口几何加载校验**：加载记忆的窗口尺寸时校验有效性，异常数值回退默认几何，避免无显示器可用时恢复出不可用窗口
- **progress 错误终态**：把 Error 设为终态，阻止后台更新线程迟到的阶段推进覆盖错误状态
- **loopback 判定统一**：启动 URL 解析与 `DSH_TAURI_HOST` 校验共用同一 loopback 主机判定，消除双实现漂移
- **标题同步启动期回退**：set_title 在主窗口尚未创建时回退到 splash 窗口，保证启动期标题同步仍生效
- **Node 安装提示修正**：npm 缺失提示改为引导从官网或 nvm 获取 Node.js 24+，不再给出装不到新版 Node 的过时 apt 命令
- **启动失败不留孤儿**：stdout/stderr 管道缺失时先终止已启动的服务再报错，不再靠 `drop(Child)` 把服务留在后台
- **会话锁投毒恢复**：supervisor 的 3 处 session 加锁改用与 progress 一致的投毒恢复，持锁线程 panic 不再让启动/退出路径二次 panic
- **退出原因文案**：exit_reason 区分正常关停与异常退出，日志不再把用户主动关闭误报成崩溃
- **单实例 socket 读超时**：聚焦请求读加超时，慢客户端不再阻塞已有实例继续聚焦
- **focus 请求写失败兜底**：给已有实例写 focus 请求失败时本实例继续启动，不再静默消失
- **通知参数统一**：notify 传参统一复用 notify_args，消除外壳与更新提示的双实现漂移
- **clipboard_write 子进程回收**：写入 wl-copy 失败时先 kill 并 wait 回收子进程，不留孤儿
- **env/home 读取收敛**：env_path 与 home_dir 统一到单一实现，shell_ui 的 HOME 读取改调共享 home_dir
- **loose_url 取首个**：按出现顺序取 http/https 中最先出现的 URL，与探针解析一致，不再固定优先 http
- **手动安装命令加引号**：手动安装命令对路径做 shell 单引号转义，含空格或单引号的路径可复制执行
- **小屏窗口几何钳制顺序**：记忆尺寸的钳制改为「屏幕上限优先、最小尺寸兜底」，小屏上不再把窗口撑得比显示器还大
- **单实例监听线程失败兜底**：监听线程 spawn 失败时记录日志、清理刚绑定的 socket 并 fail-open 继续启动，避免留下「死 socket」让后续启动静默退出
- **npm 探测超时常量化**：版本探测超时值与超时错误文案由同一常量生成，杜绝手写「30 秒」与实际超时漂移，并补上对应的真实断言测试
- **服务监控线程启动失败兜底**：服务监控线程 spawn 失败时记录日志并显示可读错误页，而不是 panic 崩溃，避免已绑定的单实例 socket 变成 stale 后让后续启动被误判为已有实例而静默退出
- **主窗口导航失败可见**：重启后主窗口已存在时导航失败不再静默吞掉，改为输出带上下文的日志（纯函数包装文案），用户停留在旧页面时日志有线索；不引入错误页跳转

**测试与质量**

- **测试整理**：删除失效的 shell-api 测试，新增 SHIM 契约单测（21 项断言）与 probe-cli 失败路径测试
- **契约漂移守卫**：新增上游桥接契约静态检查（13 项，含 SHIM 双窗口注入点、withGlobalTauri 与 dshDesktop 不实现检查）并接入 CI
- **文档测试数守卫**：校验文档声明的测试数与 Rust/npm 实际单测数一致
- **探针健壮性**：补上探针超时与信号清理
- **Rust 单测扩容**：从 17 个增至 183 个，覆盖 swap 原子替换、runtime 查找、banner loopback、native 外链与取消、通知参数、剪贴板退出状态、exec 并发与 RAII、single_instance 抢占与启动期 focus、DSH host 校验、update 版本解析与 shell_ui 原子写与启动期菜单、supervisor 优雅退出、install npm 安装参数、关停后拒绝新 spawn 的 can_spawn 门
- **错误页与启动页单测**：新增 node:vm 单测（错误页 6 项、启动页 10 项），npm 单测总数达 28 项（含重定向门禁 2 项）
- **文档契约数守卫**：文档门禁扩展为同时校验 README 与 CHANGELOG 声明的契约守卫检查项数，过期值即 FAIL
- **dshDesktop 注入守卫**：契约守卫新增禁止外壳注入 dshDesktop 全局的检查，落实「明确不实现」决策
- **clippy 警告归零**：清理 clippy 警告并把 lint 加入全量门禁，警告不再被忽略
- **七语言本地化守卫**：新增本地化资源一致性守卫（36 项），校验七语言 JSON 合法、meta 字段非空且 detail 指向对应语言文档且存在（语言错配即 FAIL）
- **文档链接守卫**：修复七语言详情页 LICENSE 死链并新增相对链接守卫（19 项，自动发现所有 md），死链即 FAIL
- **CI 门禁扩充**：把本地化校验、链接守卫与 Rust lint 接入 CI，漂移即失败
- **Tauri 命令注册完整性守卫**：只读扫描所有 `#[tauri::command]` 定义与 `main.rs` 的 `generate_handler!` 注册集合（8 项），漏注册即 FAIL；新增命令忘了接入不再到运行时才报「command not found」
- **CI 覆盖一致性守卫**：校验 `verify:fast` 的每个叶子脚本都被 CI 执行（8 项），防止本地新增检查却漏接 CI
- **check:docs 扩容至八项**：在原有测试数与契约数校验之外，增加 npm 单测数与 SHIM 契约断言数的文档声明校验，漂移即 FAIL
- **文档守卫覆盖 DESKTOP_BRIDGE 契约检查数**：check-doc-counts 新增第三处契约守卫检查数校验（DESKTOP_BRIDGE.zh.md），与 README、CHANGELOG 一起防三处数字漂移

**工程**

- **CI 增强**：接入单测与守卫，并加上 job 与步骤超时
- **桌面桥接文档**：新增 `docs/DESKTOP_BRIDGE.zh.md`，记录桌面桥接契约与「dshDesktop 明确不实现」的决策
- **开发手册**：同步新的验证命令
- **打包元数据**：`longDescription` 更新为当前架构说明
- **离线验证门禁**：新增 `verify:fast` 一键离线验证命令，脚本语法检查覆盖 tools 全部脚本
- **Node 24 对齐**：CI 与 `engines` 统一对齐 Node 24
- **一键全量验证**：新增 verify:e2e，串联全部真实端到端验证
- **版本一致性守卫**：新增版本一致性检查并接入 CI
- **文档门禁 CI**：将 check:docs 接入 CI，文档声明漂移即失败
- **二进制链接检查**：生产二进制链接检查纳入全量门禁
- **probe 失败诊断**：probe 失败时回显 stderr，便于定位运行时问题
- **契约守卫自检**：契约守卫结尾自检实际执行项数与声明常量一致，新增或删除检查项时忘记同步即 FAIL
- **npm 包描述修正**：package.json 描述改为当前架构说明，不再暗示 dshDesktop
- **探针临时目录忽略**：.gitignore 忽略探针临时目录，防止残留污染仓库
- **移除未使用的 tokio 直接依赖**：Rust 外壳未使用 tokio，去掉 `features = ["full"]` 的直接声明以缩小编译面；tauri 传递依赖仍保留
- **版本守卫覆盖锁文件**：Cargo.lock 中 dsh-tauri 包版本与 package-lock.json 顶层及 `packages[""]` 两处版本纳入一致性守卫，发版漏改任一处即 FAIL
- **CI 真探针自清理**：CI 改用自清理的 `test:clean` 跑真运行时探针，自动使用隔离临时 `DSH_HOME` 并在结束时删除，不再留下固定目录残留
- **SHIM 断言数校验纳入守卫**：文档守卫按实际 `assert.*` 调用数校验 SHIM 契约断言声明（21 项），并修正守卫正则对 camelCase 断言名的漏匹配
- **npm 探测错误按类型诊断**：npm 版本探测失败时区分正在关停、路径不存在与执行权限等错误，不再一律误报「找不到 npm」；权限类错误提示核对 `DSH_TAURI_NPM` 路径
- **验证口径同步七层改八层**：README 与开发手册的离线验证说明同步为八层（新增命令注册守卫），CI 探针描述与实际 workflow 对齐
- **环境变量表补 loopback 约束**：README 与七语言详情页共 8 处的 `DSH_TAURI_URL` 说明同步为仅 http(s)+loopback（127.0.0.1 / localhost / [::1]），不再让人误以为可指向外部服务
- **环境变量表补齐覆盖项**：README 与七语言详情页的变量表补上 `DSH_TAURI_HOST`（loopback 绑定主机）与 `DSH_TAURI_NPM`（npm 路径覆盖），与源码实际读取项一致
- **deb 托盘依赖**：deb 的 depends 补上 `libayatana-appindicator3-1`，Tauri 的 depends 是覆盖语义、托盘 dlopen 的库需显式声明，避免干净系统安装后托盘静默不可用
- **关到托盘偏好说明**：README 与七语言详情页补上 `~/.dsh/desktop-preferences.json` 的 `closeToTray` 字段位置与语义（`true` 关到托盘、`false` 退出、缺省 `true`），用户可按文档更改关窗行为
- **文档源码地图与职责对齐**：源码地图补上入口 main.rs（14 模块）；架构/源码地图/能力表中的 spawn、URL 解析、崩溃重启、SIGTERM 职责从 dsh.rs 归位到实际的 supervisor.rs / banner.rs，README、七语言详情页、开发手册与安全策略中英一致
- **桥接文档行号对齐**：DESKTOP_BRIDGE 中本仓库源码行号引用（SHIM、Rust 命令、shell_info、注入点）随源码演进同步更新，上游只读调研行号保持原样。
- **运行时查找档位对齐**：README 两处运行时查找顺序补齐第 4 档「打包资源目录 runtime/」，与开发手册及七语言详情页的四档描述一致。

## 0.4.1

- **签名发布**：补齐打包产物的签名配置与发布流程，deb / AppImage 可经验签后分发
- **Wiki 六页**：新增六页项目 Wiki，覆盖安装、使用、开发与打包等主题
- **安全配置**：加入 Dependabot 依赖更新、CODEOWNERS 代码归属与 SECURITY 漏洞披露政策，并启用 GitHub 安全功能
- **Git Flow 规范**：补充分支命名与合流规范（feature / release / hotfix）及提交约定
- **七语言本地化**：界面文案扩展为七种语言，简体中文优先，不含日语

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
