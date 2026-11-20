# 更新记录

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
