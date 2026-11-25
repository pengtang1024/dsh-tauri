# 桌面桥接契约调研（dsh-tauri ↔ 上游 Web UI）

> 调研对象：本仓库 `src-tauri/src/native.rs::SHIM` 注入的桌面全局，与 `runtime/node_modules/@deepseek-ai/dsh` 实际下发的浏览器客户端（含其依赖的 `dsh-client-*` 模块）期望的桌面桥接。
> 上游版本：`@deepseek-ai/dsh@0.2.2-rc.5`（`runtime/node_modules/@deepseek-ai/dsh/package.json`）。
> 性质：**严格 0 浸入只读调研**——没有修改 `runtime/` 下任何文件，也没有改动源码。本文只做结论沉淀，缺口只给建议。

## 0. 调研范围与证据定位：到底哪个文件在 serving 浏览器

上游 `dsh web` 真正交给浏览器的静态入口是 `@deepseek-ai/dsh-web-frontend/dist/index.html`，不是 `@deepseek-ai/dsh/lib/bin*.js` 里嵌入的内容：

- `runtime/node_modules/@deepseek-ai/dsh-web-app/src/index.ts:123-129` 的 `resolveDistIndex()` 返回 `dirname(require.resolve('@deepseek-ai/dsh-web-frontend/package.json'))/dist/index.html`；对应编译产物 `lib/index.js:77` 同款逻辑。
- `dist/index.html:13` 加载 `./assets/index-Ciz7DZxv.js`，`dist/index.html:14` 预加载 `./assets/vendor-BUMdD3V7.js`；语言高亮被切成 `dist/assets/langs/*.js` 小 chunk。
- 各 `dsh-client-*` 功能包以 `window.__ModuleLoader__.load({id:..., factory: ...})` 形式随 boot graph 动态注入浏览器（见每个包的 `lib/client.js:2` 与 `dist/assets/index-Ciz7DZxv.js.map` 的 sources），所以「上游期望的桌面桥接」必须同时看主 bundle 和这些 client 模块，不能只看 `index-*.js`。

本次使用的检索命令（在仓库根执行，均只读）：

```bash
# 找到真正下发的 bundle
grep -rn 'dsh-web-frontend' runtime/node_modules/@deepseek-ai/dsh-web-app/src/index.ts
grep -n 'script\|modulepreload' runtime/node_modules/@deepseek-ai/dsh-web-frontend/dist/index.html

# 桌面关键字全量扫——注意 --include='*.js' 会漏掉 .map 与 src，分开扫
grep -rlo '__DSH_TAURI__\|__DSH_DIRECTORY_PICKER__\|__TAURI__\|directoryPicker\|shellInfo\|protocolVersion\|dshDesktop' \
  runtime/node_modules/@deepseek-ai/dsh-web-frontend/dist --include='*.js' --include='*.map'
grep -rn '__DSH_TAURI__\|__DSH_DIRECTORY_PICKER__\|__TAURI__\|dshDesktop' \
  runtime/node_modules/@deepseek-ai --include='*.js' --include='*.ts'

# 枚举 bundle 与 client 模块里所有 globalThis.<X> 访问
grep -rho 'globalThis\.[A-Za-z_$][A-Za-z0-9_$]*' \
  runtime/node_modules/@deepseek-ai/dsh-web-frontend/dist --include='*.js' | sort | uniq -c
grep -rho 'globalThis\.[A-Za-z_$][A-Za-z0-9_$]*' \
  runtime/node_modules/@deepseek-ai/dsh-client-*/lib/client.js | sort | uniq -c
```

## 1. 注入 globals 契约表（当前 SHIM 提供了什么）

来源：`src-tauri/src/native.rs:97-136`（SHIM 原文）；对应 Rust 命令 `native.rs:26-93`；`shell_info` 返回结构见 `src-tauri/src/dsh.rs:60-68`；注入点 `src-tauri/src/shell_ui.rs:183-184` 与 `:219`（splash 与 main 两个窗口均注入）。

所有方法统一经 `globalThis.__TAURI__.core.invoke(command, args)`（`native.rs:99-103`）；桥不存在时 Promise reject `Tauri bridge unavailable`。

| global | 方法 | invoke command | 参数（JS → Rust） | 返回（Rust → JS） |
| --- | --- | --- | --- | --- |
| `__DSH_TAURI__` | `protocolVersion` | — | — | 常量 `1`（`native.rs:105`） |
| `__DSH_TAURI__` | `info()` | `shell_info` | 无 | `{ runtime, home, profile, desktopVersion }`（可能含 null 字段；`dsh.rs:60-68`） |
| `__DSH_TAURI__` | `openExternal(url)` | `open_external` | `{ url: string }` | `void`；仅允许 http(s)/mailto（`native.rs:63-69`） |
| `__DSH_TAURI__` | `notify(title, body)` | `notify` | `{ title, body }` | `void`（`native.rs:71-74`） |
| `__DSH_TAURI__.clipboard` | `readText()` | `clipboard_read` | 无 | `string`（`wl-paste --no-newline`；`native.rs:43-46`） |
| `__DSH_TAURI__.clipboard` | `writeText(text)` | `clipboard_write` | `{ text }` | `void`（`wl-copy`；`native.rs:48-60`） |
| `__DSH_DIRECTORY_PICKER__` | `pick(options)` | `pick_directory` | `{ title: options?.title }` | `string \| null`，取消为 null（`native.rs:26-41`） |
| （无 global，靠副作用） | `document.title` 同步 | `set_title` | `{ title }` | `void`；MutationObserver 观察 head，空标题忽略（`native.rs:117-135`） |

补充：`package.json:8` 的 test 脚本是 `node tools/probe-web.mjs`；与 SHIM 契约直接对应的是独立的 `test/shim-contract.test.mjs`（见第 4 节）。

## 2. 上游 Web UI 实际探测／调用的桌面全局——逐项对照

### 2.1 主 bundle（浏览器直接加载的两个 JS）

**结论：主 bundle 不引用 `__DSH_TAURI__`、`__DSH_DIRECTORY_PICKER__`、`__TAURI__`、`directoryPicker`、`shellInfo`、`protocolVersion`、`dshDesktop`（作为对象）。真实命中：**

| 命中关键字 | 文件:行 | 上下文与含义 | 对照 SHIM |
| --- | --- | --- | --- |
| `__DSH_TRANSPORT__` | `dsh-web-frontend/dist/assets/index-Ciz7DZxv.js:198` | `const i = globalThis.__DSH_TRANSPORT__` 取 `loadBundle`/`resolveAsset`，并由页面 readiness 注入 `{ownsHost:true, streamBaseUrl}` | **不涉及**：由上游 web 宿主自己写入，SHIM 不应碰 |
| `dshDesktopBoot` | 同上 `:198` | `const Ma = globalThis.dshDesktopBoot`；缺失时抛 `web boot: window.__ModuleLoader__ bootstrap facade is missing`，用于 plugin 注入/失败上报 | **不涉及**：web boot facade |
| `__DSH_BOOT__` | 同上（via `t.__DSH_BOOT__`） | 传给 `modules.create({boot: t.__DSH_BOOT__, ...})` 的 boot graph | **不涉及** |
| `navigator.clipboard.writeText` | 同上 `:129`、`:135` | `Ss(e)` 优先 `navigator.clipboard?.writeText`，失败回退 `document.execCommand`；复制消息也直接调它 | **缺失/未使用**：SHIM 提供的是 `__DSH_TAURI__.clipboard.writeText`，上游 copy 路径根本不走它；在 Tauri Wry WebView 里 `navigator.clipboard` 是否可用取决于 WebKit/权限，未验证 |
| `openExternalLink` | 同上（3 处，React context `qb`） | `BW({openExternalLink,...})` 提供 context；链接组件对 http(s) URL 调 `i?.({kind:'url',url:e})` 并 `target='_blank'` | **缺失**：SHIM 的 `openExternal` 挂在 `__DSH_TAURI__` 上，主 bundle 没有调用点；外链实际由宿主传入的 context 函数或 `_blank` 处理 |
| `__DSH_CODE_ICON_INSTANCE__*`, `__zod_globalConfig` 等 | 同上（grep `-o '__DSH_[A-Za-z0-9_]*'`） | 内部图标/DOM 缓存与库内部状态 | **不涉及** |

上述「未命中」均可用 `grep -rlo <keyword> dist --include='*.js'` 复现：`__DSH_TAURI__`、`__DSH_DIRECTORY_PICKER__`、`__TAURI__`、`directoryPicker`、`shellInfo`、`protocolVersion`、`shell_info`、`pick_directory` 在 `dist/**/*.js` 均无文件命中。

### 2.2 随 boot graph 注入的 client 模块——这里有真实桌面桥接期望

**唯一与 SHIM 命名完全一致的引用是目录选择器**（已覆盖，签名一致）：

- `dsh-client-ui-directory-picker-native/src/client/index.ts:24-28`：`globalThis.__DSH_DIRECTORY_PICKER__` 存在则 `desktop.pick()`，否则 `ctx.uiWorkspace.pickDirectory()`（Host 远程\）。
- `dsh-client-ui-directory-picker-native/src/client/flow.ts:12-15` 定义 `NativeFlowInjected { pick: () => Promise<string | null> }`；`:53-61` 处理 resolves：`null → onCancel`，字符串 → `onPicked(path)`，reject → `onError(message)`。
- 编译产物：`dsh-client-ui-directory-picker-native/lib/client.js:2` 同样含 `globalThis.__DSH_DIRECTORY_PICKER__`。
- 对照：SHIM 的 `pick(options)` 接受可选 `{title}` 并返回 `Promise<string|null>`（`native.rs:28-41, 114-116`）——**无参数调用时上游传 `undefined`，SHIM 用 `options && options.title` 兼容；返回 `null` 语义一致，已覆盖**。

**上游真正的主桌面载体是 `dshDesktop`（camelCase），SHIM 完全没有提供（缺失）：**

| 期望的 global/字段 | 证据（文件:行） | 用途 | 对照 SHIM |
| --- | --- | --- | --- |
| `globalThis.dshDesktop.protocolVersion === 1` + `dshDesktop.updates.channels` / `updates` | `dsh-client-ui-settings-general/src/client/index.ts:106-118`；产物 `lib/client.js:2` | 桌面更新源与 nightly/stable 频道切换 UI | **缺失**：SHIM 的 `protocolVersion:1` 挂在 `__DSH_TAURI__` 上，名字不同，上游读不到 |
| `dshDesktop.fileDrag` | `dsh-client-ui-file-drag/src/client/index.ts:32-33, 39`；产物 `lib/client.js:2` | 原生文件拖入 transfer，失败有反馈层 | **缺失** |
| `dshDesktop.browser` | `dsh-client-ui-sidebar-browser/src/client/index.ts:69-74`；产物 `lib/client.js:2` | 内置浏览器 tab 常驻与原生浏览器桥 | **缺失** |
| `dshDesktop.fileActions` | `dsh-client-ui-file-actions/src/client/index.ts:25-28`；产物 `lib/client.js:2` | 原文件保存、图片剪贴板等菜单动作 | **缺失** |
| `window.dshDesktop.shortcuts` / `.keyboard` | `dsh-client-shortcuts/lib/client.js:2`（`desktopShortcutStorage(window)`、`installNativeKeyboard` 与 `throw new Error('Desktop keyboard bridge unavailable')`）；`src` 同名逻辑 | 原生快捷键订阅/录制/持久化 | **缺失**；如果页面带 `data-platform` 而被判为 desktop，这里会直接抛错 |
| `globalThis.dshDesktop?.deviceInfo` | `dsh-client-ui-settings-account/src/client/index.ts` 与 `lib/client.js:2` | 反馈表单的 device_info；缺失时回退 navigator.userAgent | **缺失但有优雅降级** |
| `dshDesktop` 存在性本身（`'dshDesktop' in globalThis`） | `dsh-client-ui-settings-models/lib/client.js:2`、`dsh-client-ui-chat/lib/client.js:19`、`dsh-client-product-analytics/lib/client.js:2` | 用来区分 Desktop/Web：决定是否显示 welcome notice、默认 transcript 视图、是否开启 analytics policy 流 | **缺失**：外壳会被识别为 Web，行为与官方 Desktop 不同 |
| `globalThis.dshPlatform` | `dsh-client-ui-settings-account/lib/client.js:2` | 平台原生视图页面开关 | **缺失** |
| `globalThis.__DSH_SHORTCUTS_CONFIG__`, `__DSH_LOCALE__`, `__DSH_HOST_PATHS__`, `__DSH_FILE_UPLOAD__`, `__DSH_DOCUMENT_PREVIEW_CONFIG__` | 各 `dsh-client-*/lib/client.js:2`，由 `grep -rho 'globalThis\.[A-Za-z_$][A-Za-z0-9_$]*'` 枚举 | 其余 preload 注入：快捷键配置、语言、宿主路径、文件上传、文档预览配置 | **缺失**（部分可能有浏览器回退，未逐一验证） |
| `globalThis.__DSH_BOOT__` / `__DSH_TRANSPORT__` / `dshDesktopBoot` | `dsh-web-frontend/dist/assets/index-Ciz7DZxv.js:198`、`dsh-client-modules/lib/index.js:559`、`dsh-client-connection/lib/client.node.js:614`、`dsh-client-product-analytics/lib/client.browser.js:2` 等 | web boot graph / 流传输 / 模块加载 facade | **不涉及**：这些是 web 宿主自身注入的，不属于桌面 SHIM 契约，也不应该由 Tauri 抢注 |

**签名不符项：**没有发现同名但参数/返回不一致的情况。`__DSH_TAURI__` 在全部 `runtime/` 中 grep 无任何命中（包括 `.map` 与 src），因此它不是「上游改了签名」，而是「上游根本不认这个对象」。相对地 `protocolVersion: 1` 这个版本号语义与 `dshDesktop.protocolVersion === 1` 一致，但**载体名不同**（`__DSH_TAURI__` vs `dshDesktop`），等于白提供。

**关于「没有全局引用」的诚实结论**：对主 serving bundle（`dist/assets/index-*.js`、`vendor-*.js`、`langs/*.js`）而言，`__DSH_TAURI__`/`__TAURI__` 这些全局确实**没有任何引用**——它们在这一层属于纯增强层；但随 boot graph 加载的 `dsh-client-*` 模块确实引用了 `__DSH_DIRECTORY_PICKER__` 与 `dshDesktop.*`，所以不能把整个上游 Web UI 一概判为「不依赖桌面全局」。准确说法是：**上游使用自己的 `dshDesktop` v1 协议，本项目 SHIM 只覆盖了其中一个小子集（目录选择），其余是增强层／未对接层**。

## 3. 发现的缺口与建议（只建议，本任务不改源码）

1. **已评估并决定不实现 · 目录选择以外的桌面功能空桥（见第 5 节）。**目前只有 `__DSH_DIRECTORY_PICKER__.pick` 真正被上游 client 消费（第 2.2 节证据）。`dshDesktop.{updates,fileDrag,browser,fileActions,shortcuts,deviceInfo}`、`dshPlatform` 均未提供；用户会在外壳里看到「Web 版」行为（无原生更新频道、无原生拖入、无原生快捷键、analytics 策略流关闭等）。**决策**：这不是待补的 P0 缺口，而是明确的「不实现」——`dshDesktop` 对象一旦存在会翻转多处行为分支（最严重是 settings-account 走 desktop 登录，而项目没有任何原生 URL-open 接线），部分子桥接的部分实现还会在插件激活期抛错或吞掉全部键盘事件。完整规格、理由与硬约束见新增第 5 节；**不要**试图补 `globalThis.dshDesktop`。
2. **P1 · `__DSH_TAURI__` 与 `dshDesktop` 命名错位。**SHIM 的 `protocolVersion:1`、`info()`、`openExternal`、`notify`、`clipboard` 都是自己定义的一套，上游不读。这些调用本身无害（纯增强层），但会造成「已实现桌面能力」的错觉。**建议**：在文档/README 明确标注这部分是本项目自定义增强、不保证被上游识别。原建议「在后续任务把它们映射到 `dshDesktop` 相应字段（例如 `deviceInfo` 可复用 `info()` 的数据，`fileActions` 可只实现剪贴板写入）」**已作废，见第 5 节：不提供 `dshDesktop`、不做字段映射**，不再作为待办。
3. **P1 · 原生快捷键是潜在硬报错。**`dsh-client-shortcuts/lib/client.js:2` 在 `document.documentElement.dataset.platform` 存在时要求 `window.dshDesktop.keyboard` 并抛 `Desktop keyboard bridge unavailable`。当前 SHIM 不设置 `data-platform`，正常应走 web 分支；但如果上游宿主或 HTML 模板给 root 加了 `data-platform`，这块会崩。**建议**：验证实际 serving 的页面 root 是否带 `data-platform`；若带，需要补 keyboard bridge 或去掉该属性（不在本任务实施）。
4. **P2 · 剪贴板/外链走浏览器原生路径，SHIM 能力闲置。**主 bundle 多次使用 `navigator.clipboard.writeText`（`index-*.js:129,135`）与 React context `openExternalLink`；SHIM 的 `clipboard_write`/`open_external` 没有上游调用点。**建议**：若 Wry WebView 下 `navigator.clipboard` 权限受限，再考虑通过 `dshDesktop.fileActions` 等上游认识的通道补位；否则保持现状即可，不要重复注入。
5. **验证盲区（原待办已落地）**：`test/shim-contract.test.mjs` 只验证 SHIM 自身的行为（invoke 名称、参数、标题同步），不验证「上游是否真的调用这些 API」。**已落地，不再是待办**：原「后续加一条文档型/静态检查，把本文第 2 节的 grep 断言固化为 CI」的建议已完成——`tools/check-bridge-contract.mjs` + `npm run test:contract`（例如断言 `__DSH_DIRECTORY_PICKER__` 在 `dsh-client-ui-directory-picker-native` 里有引用），且已在 CI 中运行，避免上游版本升级后契约漂移无人发现。

## 4. 如何验证：`test/shim-contract.test.mjs` 与新文档的对应关系

本仓库当前只有 `test/shim-contract.test.mjs`（由另一轨道落盘）。该文件直接对应本文第 1 节的契约表，从源码读取 SHIM 原文并在 `node:vm` 沙箱里执行，逐条断言：

| 文档条目 | 测试断言（`test/shim-contract.test.mjs`） |
| --- | --- |
| `__DSH_TAURI__.protocolVersion = 1` 与所有 API 经 `core.invoke`（第 1 节表头） | `:65-85`：`injects __DSH_TAURI__ v1 and routes all APIs through core.invoke`，断言 `info/openExternal/notify/clipboard.readText/clipboard.writeText` → `shell_info / open_external / notify / clipboard_read / clipboard_write` 及精确 args |
| `__DSH_DIRECTORY_PICKER__.pick` → `pick_directory`、透传 `options.title`、无参调用（第 1 节倒数第 2 行） | `:87-99`：断言 `{title:'选择项目目录'}` 与 `{title:undefined}` 两次调用 |
| `set_title` 副作用：标题同步、空标题忽略、DOMContentLoaded 延迟（第 1 节最后一行） | `:101-122`、`:124-132`：MutationObserver 配置、去重、空标题忽略、loading 时推迟 |
| 桥不存在时 reject `Tauri bridge unavailable`（第 1 节开头） | `:134-139` |
| 第 2 节的上游使用证据 | **没有自动化测试覆盖**；靠本文列出的 grep 命令人工复核（见第 0 节）。这是已知验证缺口，见第 3 节建议 5 |

复现命令：

```bash
cd /home/pt/Work/DshProjects/dsh-plugin/dsh-tauri
node --test test/shim-contract.test.mjs
# 上游 grep 复核（示例）
grep -rn '__DSH_DIRECTORY_PICKER__' runtime/node_modules/@deepseek-ai/dsh-client-ui-directory-picker-native/src/client/index.ts
grep -rn 'dshDesktop' runtime/node_modules/@deepseek-ai/dsh-client-ui-{file-drag,settings-general,sidebar-browser}/src/client/index.ts
grep -o 'globalThis\.[A-Za-z_$][A-Za-z0-9_$]*' runtime/node_modules/@deepseek-ai/dsh-web-frontend/dist/assets/index-Ciz7DZxv.js | sort -u
```

> 说明（保留历史语境，附后续实测结果）：本次调研任务当时只写本文档、未运行上述测试命令；`test/shim-contract.test.mjs` 由另一轨道同时落盘，本文对其断言的引用基于直接读取源码（见 `test/shim-contract.test.mjs:65-139`）。此后 verifier 已实测全绿：`node --test` 5/5 通过、`npm run test:contract` 契约守卫 11 checks 0 FAIL、`cargo test` 113 passed；不再是「以 verifier 实际执行结果为准」的待验证状态。

## 5. dshDesktop v1 决策：明确不实现

> 本章是精读 `runtime/node_modules/@deepseek-ai/dsh-client-*` 源码后的最终裁决，优先级高于第 2.2 节的「缺失」清单和第 3 节的历史建议。**结论：本项目不提供 `globalThis.dshDesktop`，也不提供任何子桥接的部分实现。**第 3 节第 2 条中「把能力映射到 dshDesktop 字段」的设想一并作废，不作为待办。

### 5.1 核心澄清：`dshDesktop` 存在本身不会触发快捷键硬报错

- 硬报错的真实门控是 `document.documentElement.dataset.platform`：`dsh-client-shortcuts/src/client/dom.ts:13-21` 仅在该属性存在时才把 runtime 设为 `'desktop'`；`dsh-client-shortcuts/src/client/index.ts:43-46` 才抛 `'Desktop keyboard bridge unavailable'`。
- SHIM（`src-tauri/src/native.rs`）与 served `index.html` 都没有写 `data-platform`（grep writers = 0），所以当前外壳走 Web 分支，不会命中这条硬报错。
- **绝对不要设置 `document.documentElement.dataset.platform`**，否则立即触发上述硬报错。

### 5.2 但 `dshDesktop` 存在会翻转 4 处行为分支（不抛错，是行为回归）

| 分支 | 证据 | 翻转后的行为 |
| --- | --- | --- |
| analytics policy 流开启 | `dsh-client-product-analytics/src/client/index.ts:29` | 从 Web 行为切到 Desktop 行为 |
| welcome notice 关闭 | `dsh-client-ui-settings-models/src/client/index.ts:82,147` | 用户不再看到欢迎提示 |
| transcript 默认 standard | `dsh-client-ui-chat/src/client/apply.ts:172` | 默认视图从 detailed 变 standard |
| **desktop 登录** | `dsh-client-ui-settings-account/src/client/index.ts:48,229-293` | 走 RPC `startSignIn`，不再 `window.open` 弹授权页；项目当前**没有任何原生 URL-open 接线**，用户可能永远看不到授权页——**最严重风险** |

### 5.3 精确规格表

字段一栏均为 `dshDesktop` 的属性（除单独标注的其它 global）。「缺失行为」指不提供该字段时上游的表现；「结论」列是本项目的裁决。

| 字段 | 读取表达式 | 类型 | 缺失行为 | 是否有 Rust 支撑 | 结论 |
| --- | --- | --- | --- | --- | --- |
| `protocolVersion` | `globalThis.dshDesktop?.protocolVersion === 1`（settings-general `src/client/index.ts:106-109`、file-drag `:32-33`、sidebar-browser `:69-72`） | 必须恰为数字 `1` | 优雅 Web 回退（调用点判空后走 Web 分支） | 现有 Rust 无法安全利用：SHIM 的 `__DSH_TAURI__.protocolVersion` 载体名不同，上游读不到 | 不实现 |
| `updates` | settings-general `src/client/index.ts:106-121`，类型 `src/types.ts:30-35` | `{ channels?: { get(): Promise<DesktopChannelState>; set('nightly'\|'stable'): Promise<DesktopChannelChangeResult>; subscribe(cb): () => void }; status(): Promise<DesktopUpdatePresentation>; open(): Promise<void>; subscribe(cb): () => void }` | 不显示原生更新频道/状态 UI，Web 回退，优雅 | 无。构造函数 `desktop-update-source.ts:16`、`desktop-channel-source.ts:25,30` 激活期立即调 `subscribe`，部分实现 → TypeError | 绝对不碰 |
| `fileDrag` | `globalThis.dshDesktop?.fileDrag`（file-drag `src/client/index.ts:32-39`、`transfers.ts:37-113`） | `{ prepare(source, requestId): Promise<handle \| undefined>; cancel(requestId): void; start(handle): Promise<boolean>; release(handle): void }` | blob-URL 浏览器拖放回退 | 无 | 不碰 |
| `browser` | sidebar-browser `src/client/index.ts:69-101`，类型 `src/types.ts:20-27` | `{ acquire(workspace): Promise<{ lease, partition }>; release(lease): Promise<void>; onOpenRequested(lease, cb): () => void }` | 回退 iframe | 无。Tauri 源码零 `.emit`/`.listen`，无法向页面推送事件 | 不碰 |
| `fileActions` | file-actions `src/client/index.ts:25-28`，类型 `src/types.ts:15-27` | `{ save({ url, name?, mediaType?, mode: 'download'\|'saveAs' }): Promise<{ status: 'cancelled'\|'requested'\|'saved'; path?: string }>; readClipboardImage(url): Promise<{ bytes: Uint8Array; mediaType: string }> }` | `canSaveAs = false`；`saveAs` 抛 `'Save as requires Desktop'`（`files.ts:52`） | 无 save-image 命令；`clipboard_write` 仅文本。部分实现翻 `canSaveAs = true` 后产生可见失败 | 不碰 |
| `shortcuts` / `keyboard` | `window.dshDesktop?.keyboard`（shortcuts `src/client/index.ts:45`）、`window.dshDesktop?.shortcuts`（`storage.ts:39`） | Keyboard `{ subscribe(cb(input): void): () => void; closeWindow(revision): Promise<void> }`（`src/native.ts:20-29`）；Shortcuts `{ get(defs): Promise<snapshot>; edit(edit, revision): Promise<result>; subscribe(cb): () => void; recording(bool): Promise<void> }`（`src/persistence.ts:33-38`） | Web 键盘派发路径，优雅 | 无。关键：`dom.ts:91` 的 `if (native) return` 意味着空 `subscribe` 会吞掉全部键盘派发 | 绝对不碰 |
| `deviceInfo` | `globalThis.dshDesktop?.deviceInfo`（settings-account `src/client/index.ts:198-200`） | `() => Promise<string>` | `navigator.userAgent` 回退（`:201-204`），异常被吞掉（`:206-213`） | 唯一可用现有 `shell_info` 支撑的字段 | 因 `dshDesktop` 整体不实现，本次也不加 |
| `dshPlatform` | settings-account `:157`、`PlatformOverlay.tsx:10-17` | `{ open(page: 'usage'\|'top-up', bounds): Promise<void>; setBounds(bounds): Promise<void>; close(): Promise<void> }` | 功能缺席，优雅 | 无；需要原生内嵌视图 | 不碰 |

### 5.4 其它全局速览

| global | 证据 | 风险 | 结论 |
| --- | --- | --- | --- |
| `__DSH_SHORTCUTS_CONFIG__` | shortcuts `index.ts:51`、`config.ts:5-13`，`{ stopSequenceMs: number }`，默认 500 | 相对安全 | 本任务不改 |
| `__DSH_LOCALE__` | locale `src/client/index.ts:568` | 会让 `apply()` 变 async，数据坏时硬抛 TypeError（`bootstrap.ts:24-31`） | 不碰 |
| `__DSH_HOST_PATHS__` | conversation `apply.ts:113-115,504-521`，`pathFor(file): string` | 无法从浏览器 `File` 实现 | 不碰 |
| `__DSH_FILE_UPLOAD__` | file-upload `runtime.ts:170`，`{ fetch }` | 非必需，worker 回退够用 | 不碰 |
| `__DSH_DOCUMENT_PREVIEW_CONFIG__` | documentpreview `index.ts:90` | 有完整默认值（`config.ts:29-41`），无害 | 不碰 |

### 5.5 决策与硬约束

**决策：明确不提供 `dshDesktop`。**没有任何子桥接的部分实现是安全的——要么在插件激活期抛错（`updates`），要么吞掉全部键盘派发（`shortcuts`/`keyboard`），要么翻转登录路径导致授权页不可达（`settings-account`）。

硬约束清单：

1. **绝不设置 `document.documentElement.dataset.platform`**——会触发 `'Desktop keyboard bridge unavailable'` 硬报错（见 5.1）。
2. **绝不部分实现子桥接**——`updates` / `fileDrag` / `browser` / `fileActions` / `shortcuts` / `keyboard` / `deviceInfo` / `dshPlatform` 任何一项都不加；半套实现比完全缺失更危险。
3. 不碰 `__DSH_LOCALE__`、`__DSH_HOST_PATHS__` 等高风险全局；不为对齐 Desktop 改动 `runtime/`、源码、`package.json`、`tools/`。
4. 维持现状：只保留已覆盖的 `__DSH_DIRECTORY_PICKER__` 与 shell 增强全局，其余走上游自带的 Web 回退路径。

### 5.6 未来若要对齐 Desktop 的前置条件

按当前 `src-tauri/src` 源码**没有任何 `.emit`/`.listen`** 的事实，以下缺一不可；在它们全部落地前，维持「不实现」是唯一安全选择：

1. **原生事件推送通道**（Rust → WebView）：`updates.status`/`updates.channels`、`browser.onOpenRequested`、`shortcuts.subscribe`、`keyboard.subscribe` 等所有 `subscribe` 回调都依赖它。
2. **原生 URL-open 接线**：支撑 `settings-account` 的 desktop 登录（`startSignIn`）授权页；现有 `open_external` 只是 SHIM 自用全局，没有接到登录流程。
3. **原生快捷键订阅**：含录制（`recording`）、持久化（`get`/`edit`）与 `closeWindow`，并保证 `subscribe` 不吞事件。
4. **原生保存 / 拖入 / 浏览器视图命令**：`fileActions.save`、`readClipboardImage`、`fileDrag` 四方法、`browser.acquire`/`release`。
5. **原生更新频道/状态实现与内嵌平台视图**：`updates` 全套与 `dshPlatform.open`/`setBounds`/`close`。
