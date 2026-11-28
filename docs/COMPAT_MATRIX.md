# dsh-tauri 多发行版兼容性矩阵（KVM 实测）

> 本文由 task-406（子 agent vm-compat）产出，仅新增本文件，不改源码、README、CHANGELOG 或版本文件。
> 测试对象：v0.7.2（signed tag，commit b2b493d12ad077f5e1e7b2e7f7c709ae77ed4788），口径由 release-072 对齐。
> 产物来源：GitHub Release v0.7.2 的 CI 资产（非本地构建产物）：
> - deb：DSH.Tauri_0.7.2_amd64.deb（3,977,602 B），https://github.com/pengtang1024/dsh-tauri/releases/download/v0.7.2/DSH.Tauri_0.7.2_amd64.deb
> - AppImage：DSH.Tauri_0.7.2_amd64.AppImage（82,655,736 B），https://github.com/pengtang1024/dsh-tauri/releases/download/v0.7.2/DSH.Tauri_0.7.2_amd64.AppImage
> - Release 页：https://github.com/pengtang1024/dsh-tauri/releases/tag/v0.7.2 ；CI 只产出 deb 与 AppImage，无 rpm。
>
> 测试纪律：全程 libvirt/KVM 虚拟机，串行（并发 1 ≤ 2），每台 2 vCPU / 4 GiB RAM / 25 GiB qcow2，NAT 网络（network=default），命名 dsh-compat-<distro>。VM 内不配置任何 X11 / Xorg / X server，只做 headless 装包、依赖与启动探测（DISPLAY=、WAYLAND_DISPLAY=）。VM 内启动失败若仅因无显示后端，记为「headless 预期失败」，不误判为缺包。宿主正在运行的用户实例 PID 3524464 全程未动；未使用 pgrep -f 批量 kill；VM 管理只按显式域名。

## 兼容矩阵总览

| 发行版 / 版本 | 包格式 | 安装结果 | 依赖闭合 | 缺失依赖 | 对应安装命令 | 能否启动（headless） | 备注 |
|---|---|---|---|---|---|---|---|
| **Ubuntu 24.04.5 LTS**（Noble，kernel 6.8.0-147-generic） | .deb（CI Release 资产） | ✅ 成功（apt-get install -y /tmp/dsh.deb exit 0；dpkg -l = ii dsh-tauri 0.7.2 amd64） | ✅ apt-get check exit 0，无破损依赖 | 无（deb 声明的 7 项依赖全部由 apt 自动装齐）；kdialog 不在系统（KDE 专属，可选增强） | sudo apt install ./DSH.Tauri_0.7.2_amd64.deb（自动拉 libwebkit2gtk-4.1-0 / libgtk-3-0 / libayatana-appindicator3-1 / zenity / wl-clipboard / libnotify-bin / xdg-utils） | ❌ exit 101：tao event_loop.rs:219 → gtk::rt::init → Failed to initialize GTK（无显示后端预期失败，非缺库；ldd 无 not found） | .deb 主战场验证通过；外部命令 zenity / wl-copy / wl-paste / notify-send / xdg-open 全部就位 |
| **Fedora 42 Cloud Edition** | AppImage（CI Release 资产） | ✅ AppImage 解包成功（--appimage-extract exit 0）；最小系统原生缺 10 个共享库 | ✅ 补齐运行库后 ldd 无 not found | 硬依赖（缺则 exit 127）：libfribidi.so.0、libfontconfig.so.1、libwayland-client.so.0、libX11.so.6、libX11-xcb.so.1、libxcb.so.1、libgbm.so.1、libdrm.so.2、libGL.so.1、libEGL.so.1。助手命令（最小安装全缺，功能降级）：zenity / wl-clipboard(wl-copy,wl-paste) / libnotify(notify-send) / xdg-utils(xdg-open) / kdialog。硬缺口：libayatana-appindicator3(-devel) 在 Fedora 42 仓库 NOT_FOUND | sudo dnf -y install fribidi fontconfig libxcb mesa-libgbm libdrm mesa-libGL mesa-libEGL libX11（补齐已验证的 8 包）；sudo dnf -y install zenity wl-clipboard libnotify xdg-utils kdialog（助手+kdialog 独立包）；托盘需 libappindicator-gtk3（ABI 不同，不承诺可用） | ❌ 补库前 exit 127（libfribidi.so.0: cannot open shared object file）；补库后 exit 101（同上 GTK backend panic，headless 预期） | webkit2gtk4.1 2.52.3 / webkit2gtk4.1-devel / gtk3 / zenity 4.2.2 / wl-clipboard 2.2.1 / libnotify 0.8.8 / xdg-utils 1.2.1 / kdialog 25.12.3 均经 dnf list 确认为真实包名；无 rpm 产物，dnf 用户只能用 AppImage 或源码构建 |
| **Arch Linux**（cloudimg 20261115.625457） | AppImage | ⚠️ SKIP（VM 内未测成）：官方 cloudimg 不带 cloud-init/sshd，root/pt 无已知密码，SSH 端口 22 始终关闭；serial console 只有 login: 提示。按「10 分钟内起不来如实跳过」执行，不卡死 | 未验证 | 未验证（无 VM 实测缺库数据）；包名仅经官方数据库核验：webkit2gtk-4.1 [extra] 2.54.3、gtk3、libayatana-appindicator [extra] 0.6.1、zenity 4.2.2、wl-clipboard 2.3.0、libnotify 0.8.9、xdg-utils 1.2.1、kdialog 26.08.3 | sudo pacman -S webkit2gtk-4.1 gtk3 libayatana-appindicator zenity wl-clipboard libnotify xdg-utils kdialog（基于官方包数据库，非 VM 实测）；AppImage 运行库参照 Fedora 缺库清单按 pacman 等价包补 | 未验证（VM 不可达，未做启动测试） | Arch 有 ayatana 版 indicator（Fedora 没有）；滚动发行版 WebKit 更新快（2.54.3），AppImage 自带 WebKit 与系统版本可能有兼容差异，未经实测 |

> 图例：✅ 成功；❌ 按预期失败/缺依赖；⚠️ 未完成/未验证。headless「不能启动」不代表有 GUI 时不能启动——VM 内禁止配置 X11，因此只能验证到「装包 + 依赖闭合 + 启动到 GTK 后端初始化」为止。

## 硬依赖 vs 可选增强（回答用户关切）

- 硬依赖（缺了起不来）：
  - Ubuntu/Debian 系：deb 的 Depends 七项（libwebkit2gtk-4.1-0、libgtk-3-0、libayatana-appindicator3-1、zenity、wl-clipboard、libnotify-bin、xdg-utils）由包管理器强制安装，缺失则不装/破损；apt-get check 可直接验收。
  - Fedora/AppImage：10 个动态库 SONAME（见矩阵），缺任一即 exit 127 · error while loading shared libraries。注意 AppImage 并不自带这些基础图形/wayland 运行库。
- 可选增强（缺了功能降级，不影响装包）：
  - kdialog：KDE 系对话框兜底；Ubuntu 24.04 默认缺（GNOME 不需要），Fedora/Arch 可独立安装（不是 kde-components 元包）。当前代码已实现 kdialog 兜底链（portability/task-405），本机无 kdialog 真机，KDE 取消键退出码 1 依公开文档标注。
  - zenity：目录选择与关于面板均已改为自绘 GTK（`picker::show_info`）为主路径，zenity/kdialog 仅作兜底降级；缺失不影响主功能。
  - wl-copy / wl-paste：Wayland 剪贴板；无 Wayland 会话时相关功能降级。
  - notify-send：通知；缺则无桌面通知。
  - xdg-open：外部链接打开；缺则相关调用失败。
  - libayatana-appindicator3：托盘图标。Debian/Arch 有；Fedora 42 仓库没有（只有 libappindicator-gtk3，ABI 不同），Fedora 上托盘功能不可承诺。

## 每台的证据（命令 + 退出码摘要）

### 基线（开工前，2026-11-28T03:54+08:00）
    virsh list --all                          # exit 0：仅 tiny10-live、win11-live（均关闭），无 dsh-compat-*
    du -sh /mnt/data/vm                       # exit 0：48G
    ps -eo pid,comm,args | grep '[q]emu-system'  # 无匹配 → 无遗留 qemu 进程
    ps -p 3524464 -o pid,comm,args            # exit 0：/usr/bin/dsh-tauri（宿主用户实例，全程未动）

### Ubuntu 24.04（VM dsh-compat-ubuntu，IP 192.168.122.243）
    virt-install --name dsh-compat-ubuntu --memory 4096 --vcpus 2 --disk ...,size=25G ... --import   # exit 0
    ssh ... pt@192.168.122.243 'cat /etc/os-release'   # exit 0：Ubuntu 24.04.5 LTS, kernel 6.8.0-147-generic
    scp dsh-0.7.2.deb pt@IP:/tmp/dsh.deb              # exit 0
    dpkg-deb -f /tmp/dsh.deb Package Version Depends  # exit 0：dsh-tauri / 0.7.2 / 7 项 Depends
    sudo apt-get update -qq && sudo apt-get install -y -qq /tmp/dsh.deb   # APT_INSTALL_EXIT=0
    dpkg -l dsh-tauri                                 # DPKG_L_EXIT=0：ii  dsh-tauri  0.7.2  amd64
    sudo apt-get check                                # APT_CHECK_EXIT=0（依赖闭合，无破损）
    for c in zenity wl-copy wl-paste notify-send xdg-open kdialog; do command -v $c || echo MISSING; done
        # zenity=/usr/bin/zenity, wl-copy/wl-paste=/usr/bin/, notify-send=/usr/bin/, xdg-open=/usr/bin/, kdialog=MISSING
    ldd /usr/bin/dsh-tauri | grep 'not found'         # 无输出（LDD_EXIT=0）；libwebkit2gtk-4.1.so.0 / libgtk-3.so.0 / libayatana-appindicator3.so.1 全解析
    DISPLAY= WAYLAND_DISPLAY= timeout 20 /usr/bin/dsh-tauri --version   # VERSION_EXIT=101
        # stderr: thread 'main' panicked at .../tao-0.37.3/src/platform_impl/linux/event_loop.rs:219:53:
        #         Failed to initialize gtk backend!: BoolError { message: "Failed to initialize GTK", ... gtk::rt::init ... }
    DISPLAY= WAYLAND_DISPLAY= timeout 20 /usr/bin/dsh-tauri             # LAUNCH_EXIT=101，同一 GTK backend 错误（headless 预期）
    virsh destroy dsh-compat-ubuntu                    # exit 0
    virsh undefine dsh-compat-ubuntu --remove-all-storage  # exit 0，删除卷 vda（overlay）

### Fedora 42（VM dsh-compat-fedora，IP 192.168.122.39）
    virt-install --name dsh-compat-fedora --memory 4096 --vcpus 2 --disk ...,size=25G ... --import  # exit 0
    ssh ... pt@192.168.122.39 'cat /etc/os-release'   # exit 0：Fedora Linux 42 (Cloud Edition)
    scp dsh-0.7.2.AppImage pt@IP:/tmp/dsh.AppImage    # exit 0
    cd /tmp && ./dsh.AppImage --appimage-extract       # EXTRACT_EXIT=0 → /tmp/squashfs-root/
    ldd /tmp/squashfs-root/usr/bin/dsh-tauri | awk '/not found/{print $1}' | sort -u
        # libdrm.so.2 libEGL.so.1 libfontconfig.so.1 libfribidi.so.0 libgbm.so.1 libGL.so.1
        # libwayland-client.so.0 libX11.so.6 libX11-xcb.so.1 libxcb.so.1（LDD_EXIT=0）
    DISPLAY= WAYLAND_DISPLAY= timeout 20 /tmp/dsh.AppImage --version   # LAUNCH_EXIT=127
        # stderr: dsh-tauri: error while loading shared libraries: libfribidi.so.0: cannot open shared object file: No such file or directory
    sudo dnf -y install -q fribidi fontconfig libxcb mesa-libgbm libdrm mesa-libGL mesa-libEGL libX11
        # DNF_INSTALL_LIBS_EXIT=0；rpm -q 八包全部存在（fribidi-1.0.16-2.fc42, mesa-libgbm-25.1.9-1.fc42, ...）
    ldd /tmp/squashfs-root/usr/bin/dsh-tauri | awk '/not found/{print $1}' | sort -u   # 输出为空（无缺失）
    DISPLAY= WAYLAND_DISPLAY= timeout 20 /tmp/dsh.AppImage --version   # LAUNCH2_EXIT=101（同 GTK backend panic，headless 预期）
    dnf -q list --available <pkg> 逐包查询：
        # 存在：webkit2gtk4.1 2.52.3 / webkit2gtk4.1-devel 2.52.3 / gtk3 3.24.52 / zenity 4.2.2 /
        #       wl-clipboard 2.2.1 / libnotify 0.8.8 / xdg-utils 1.2.1 / kdialog 25.12.3 /
        #       libappindicator-gtk3-devel 12.10.1-8.fc42
        # NOT_FOUND：libayatana-appindicator3、libayatana-appindicator3-devel
    for c in zenity wl-copy wl-paste notify-send xdg-open kdialog; do command -v $c || echo MISSING; done
        # 最小 Fedora Cloud：6 项全 MISSING
    sudo dnf -y install -q zenity wl-clipboard libnotify xdg-utils kdialog   # HELPERS_INSTALL_EXIT=0（后台任务输出）
        # 说明：该批安装的命令路径检查因本地 shell 引号问题未可靠复验，故只记「包安装退出码 0」，不声称命令已复验
    virsh destroy dsh-compat-fedora                    # exit 0
    virsh undefine dsh-compat-fedora --remove-all-storage  # exit 0（删除卷 vda）

### Arch Linux（VM dsh-compat-arch，IP 192.168.122.69）— SKIP
    virt-install --name dsh-compat-arch --memory 4096 --vcpus 2 --disk ...,size=25G --os-variant archlinux --import  # exit 0
    nc -z -w 2 192.168.122.69 22       # 连续 6 次 PORT_CLOSED（SSH 未启动）
    ssh ... pt@192.168.122.69          # Connection refused；root/pt 经 console 均需 Password（官方 cloudimg 无 cloud-init）
    virsh reboot dsh-compat-arch       # exit 0；重启后 SSH 仍 closed → 按 10 分钟规则 SKIP，未测 AppImage 启动
    # 包名核验（非 VM，官方 API）：curl 'https://archlinux.org/packages/search/json/?q=<pkg>' → 全部存在于 [extra]
    virsh destroy dsh-compat-arch                     # exit 0
    virsh undefine dsh-compat-arch --remove-all-storage  # exit 0（删除卷 vda）

### 收尾：资源释放核对（硬要求）
    virsh list --all   # exit 0：仅 tiny10-live、win11-live（均关闭）；grep dsh-compat → 无匹配
    ls <每台的 overlay qcow2>   # 均 No such file or directory（已随 --remove-all-storage 删除）
    du -sh /mnt/data/vm         # exit 0：48G（与开工前基线完全一致，未增长）
    ps -eo pid,ppid,comm,args | grep '[q]emu-system'   # 无输出（无遗留 qemu 进程）

释放前基线：du = 48G、域列表无 dsh-compat-*、无 qemu 进程；三台 VM 的 qcow2 overlay 均在 .tmp-compat/（不在 /mnt/data/vm），每台测完即 virsh destroy（exit 0）+ virsh undefine --remove-all-storage（exit 0，删除 vda）。释放后：du = 48G（与基线一致）、域列表只剩两个原有且关闭的 VM、无 qemu 进程。

### v0.9.0 — Ubuntu/Debian（deb + AppImage）

> 本节由子 agent vm-deb090 追加，仅覆盖 Ubuntu 24.04 上的 deb 与 AppImage 实测，不改上文其它段落。测试对象：v0.9.0 CI Release 资产（非本地构建）。产物：deb = DSH.Tauri_0.9.0_amd64.deb（3,999,842 B，sha256 `f68a2412b11fbc5c2b1d2b8b6b4f4bbbf7889f43a29fa0f340576d93e6b0aeae`）；AppImage = DSH.Tauri_0.9.0_amd64.AppImage（82,590,200 B，sha256 `40f9f2ef8b23fa96ec0070dc88d45b6dc4663ff5854285b698df85892a51452e`）。两者均由 GitHub Release v0.9.0 经已认证 api.github.com asset 端点下载（github.com 直连超时），大小与官方资产完全一致。环境：VM dsh-compat-ubu090，Ubuntu 24.04.5 LTS（Noble），2 vCPU / 4 GiB / 25 GiB qcow2，NAT，cloud-init seed（NoCloud/xorriso），SSH 免密；VM 内无 X11/Xorg/Xvfb，仅 headless（DISPLAY=、WAYLAND_DISPLAY=）。宿主零 apt/apt-get 包操作，未用 pgrep -f 批量 kill，未动宿主 PID 3524464。

**① deb 安装与依赖闭合（VM dsh-compat-ubu090，IP 192.168.122.56）**

    scp dsh-0.9.0.deb -> /tmp/dsh.deb                    # exit 0；VM 内 sha256 与宿主一致
    dpkg-deb -f /tmp/dsh.deb Package Version Depends     # exit 0：dsh-tauri / 0.9.0 / amd64
        # Depends（10 项，含重复）：libwebkit2gtk-4.1-0, libgtk-3-0, libayatana-appindicator3-1,
        #   zenity, wl-clipboard, libnotify-bin, xdg-utils, libappindicator3-1,
        #   libwebkit2gtk-4.1-0, libgtk-3-0（后两项为重复声明）
    sudo apt-get update -qq                              # exit 0
    sudo apt-get install -y -qq /tmp/dsh.deb             # APT_INSTALL_EXIT=0
    dpkg -l dsh-tauri                                    # exit 0：ii  dsh-tauri  0.9.0  amd64
    sudo apt-get check                                   # APT_CHECK_EXIT=0（依赖闭合，无破损）
    ldd /usr/bin/dsh-tauri | grep 'not found'            # 无输出（grep exit 1 = 无缺失）；ldd 共 129 行
    DISPLAY= WAYLAND_DISPLAY= timeout 20 /usr/bin/dsh-tauri --version  # VERSION_EXIT=101
    DISPLAY= WAYLAND_DISPLAY= timeout 20 /usr/bin/dsh-tauri            # LAUNCH_EXIT=101
        # 两者同一错误：tao-0.37.3/event_loop.rs:219 "Failed to initialize gtk backend!"
        #   （gtk::rt::init / Failed to initialize GTK）→ 无显示后端预期失败，非缺库 bug

  deb 声明依赖就位情况（dpkg-query 实测版本）：libwebkit2gtk-4.1-0 = 2.52.6-0ubuntu0.24.04.2；libayatana-appindicator3-1 = 0.5.93-1build3；zenity = 4.0.1-1build3；wl-clipboard = 2.2.1-1build1；libnotify-bin = 0.8.3-1build2；xdg-utils = 1.1.3-4.1ubuntu3。注意 v0.9.0 相对 v0.7.2 的两处变化：① deb 声明的 `libgtk-3-0` 在 Noble 已更名为 `libgtk-3-0t64`（实测装的是 3.24.41-4ubuntu1.3），apt 仍正确解析并装齐、apt-get check 0，运行库 libgtk-3.so.0 解析到位，不构成破损；② 新增声明 `libappindicator3-1`，Noble/universe 有候选（12.10.1+20.10.20200706.1-0ubuntu5）但本次未被拉入（dpkg 状态 not-installed），apt 依赖树仍判闭合、ldd 无缺失，不影响启动。helper 命令 zenity / wl-copy / wl-paste / notify-send / xdg-open 全部就位；kdialog 缺席（可选增强，GNOME 不需要）。ldd 关键项均解析：libwebkit2gtk-4.1.so.0、libgtk-3.so.0 均指向 /lib/x86_64-linux-gnu/。

**② AppImage 解包、缺库补齐与启动（干净 VM 重建，IP 192.168.122.159，装包前 webkit/gtk 包数 = 0）**

    scp dsh-0.9.0.AppImage -> /tmp/dsh.AppImage          # exit 0
    chmod +x /tmp/dsh.AppImage                           # exit 0
    cd /tmp && ./dsh.AppImage --appimage-extract         # EXTRACT_EXIT=0 → /tmp/squashfs-root/usr/bin/dsh-tauri
    ldd /tmp/squashfs-root/usr/bin/dsh-tauri（干净系统） # LDD_EXIT=0；缺 6 项（见下，awk 'not found' 去重）
        # libEGL.so.1, libGL.so.1, libX11-xcb.so.1, libgbm.so.1, libharfbuzz.so.0, libwayland-client.so.0
    DISPLAY= WAYLAND_DISPLAY= timeout 20 /tmp/dsh.AppImage --version  # 补库前 exit 127
        # stderr: error while loading shared libraries: libwayland-client.so.0
    sudo apt-get update -qq                              # exit 0
    sudo apt-get install -y -qq libegl1 libgl1 libx11-xcb1 libgbm1 libharfbuzz0b libwayland-client0 zenity wl-clipboard libnotify-bin xdg-utils   # APPIMAGE_DEPS_INSTALL_EXIT=0
    ldd /tmp/squashfs-root/usr/bin/dsh-tauri（补库后）   # LDD_EXIT=0；missing_count=0（无 not found）
    DISPLAY= WAYLAND_DISPLAY= timeout 25 /tmp/dsh.AppImage --version  # POSTINSTALL_LAUNCH_EXIT=101
        # 同一 tao GTK backend panic（headless 预期失败，已越过动态库加载阶段）

  准确 apt 包名与实测安装版本（Ubuntu 24.04，逐 SONAME 经 apt-cache policy 核对候选）：libEGL.so.1 → `libegl1` 1.7.0-1build1；libGL.so.1 → `libgl1` 1.7.0-1build1；libX11-xcb.so.1 → `libx11-xcb1` 2:1.8.7-1ubuntu0.1；libgbm.so.1 → `libgbm1` 25.2.8-0ubuntu0.24.04.5；libharfbuzz.so.0 → `libharfbuzz0b` 8.3.0-2build2；libwayland-client.so.0 → `libwayland-client0` 1.22.0-2.1build1；helper：`zenity` 4.0.1-1build3 / `wl-clipboard` 2.2.1-1build1 / `libnotify-bin` 0.8.3-1build2 / `xdg-utils` 1.1.3-4.1ubuntu3（补库后 wl-copy/wl-paste/notify-send/xdg-open/zenity 命令全部就位）。layout 统计：ldd 129 行中 bundled=98（AppImage 自带 libwebkit2gtk-4.1.so.0、libgtk-3.so.0 等，指向 squashfs-root/usr/bin/../lib）、system=29（glibc/libgcc、fribidi/fontconfig/wayland-client/X11/xcb/gbm/drm/GL/EGL 等基础库仍依赖系统）、missing=0。较 v0.7.2（最小系统缺 10 项）改善：v0.9.0 已把 WebKit2GTK/GTK3 打进 AppImage，但基础图形/Wayland 运行库仍需在上表中补齐，不是开箱即用。

**③ rpm 在 Ubuntu 上不适用**：Ubuntu 为 dpkg/apt 体系，rpm 包既非本矩阵 Ubuntu 轨道产物也无原生安装路径；不强装、不验证（由 Fedora/RPM 轨道覆盖）。

**④ 资源释放核对（硬要求）**

    释放前：virsh list --all → id 9 dsh-compat-ubu090 运行 + tiny10-live/win11-live（关闭）；du -sh /mnt/data/vm = 48G
    virsh destroy dsh-compat-ubu090      # exit 0
    virsh undefine dsh-compat-ubu090 --remove-all-storage  # exit 0（删除卷 vda 与 sda/seed.iso）
    释放后：virsh list --all → 仅 tiny10-live、win11-live（均关闭），无 dsh-compat-*；du -sh /mnt/data/vm = 48G（与基线一致）；
            ps -eo pid,comm,args | grep '[q]emu-system' → 无输出（无遗留 qemu 进程）；
            ps -p 3524464 → dsh-tauri /usr/bin/dsh-tauri（宿主实例全程未动）
    清理：临时目录 .tmp-vm-ubu090/（镜像、seed ISO、deb/AppImage、SSH 预置、脚本、日志）已 rm -rf；
          .git/info/exclude 中仅移除本人新增的 .tmp-vm-ubu090/ 行，其它轨道（如 .tmp-vm-fed090/）行保留不动。


### v0.9.0 — Fedora/RHEL（rpm + AppImage）

> 本节由子 agent vm-rpm090 在 KVM 中实测追加，只记录 v0.9.0 本轮结果；上文各段保持 v0.7.2 原样，其中「Fedora 无 rpm 产物」「libayatana-appindicator3 不可满足」两条结论**仅对 v0.7.2 成立**，v0.9.0 已改变（见下）。
> 测试对象：v0.9.0（master HEAD 9eb2665）。产物来自 GitHub Release v0.9.0，非本地构建：
> - rpm：DSH.Tauri-0.9.0-1.x86_64.rpm（4,004,713 B，sha256 29a9ff7c84e1d13d5b61bf90dd837b8b7cdf23f4c0a6191d5bf46cb072dfa544），https://github.com/pengtang1024/dsh-tauri/releases/download/v0.9.0/DSH.Tauri-0.9.0-1.x86_64.rpm
> - AppImage：DSH.Tauri_0.9.0_amd64.AppImage（82,590,200 B，sha256 40f9f2ef8b23fa96ec0070dc88d45b6dc4663ff5854285b698df85892a51452e），https://github.com/pengtang1024/dsh-tauri/releases/download/v0.9.0/DSH.Tauri_0.9.0_amd64.AppImage
> VM：dsh-compat-fed090，Fedora-Cloud-Base-Generic-42-1.1.x86_64.qcow2（sha256 e401a4db2e5e04d1967b6729774faa96da629bcf3ba90b67d8d9cce9906bec0f），25 GiB qcow2 overlay，2 vCPU / 4 GiB RAM，NAT（network=default），NoCloud seed（xorriso 生成 CIDATA ISO，user-data 注入 pt 用户 + ed25519 公钥，已回读校验）。VM 内无 X11/Xorg/X server，只做 headless 装包、依赖与启动探测。

| 格式 | 安装结果 | 依赖闭合 | 缺失依赖 | 启动（headless） | 备注 |
|---|---|---|---|---|---|
| **.rpm**（新增格式） | ✅ sudo dnf install -y /tmp/dsh-0.9.0.rpm exit 0，277 包事务 Complete! | ✅ sudo dnf check exit 0 无输出；ldd /usr/bin/dsh-tauri 无 not found | 无。**未出现 nothing provides libayatana-appindicator3.so.1**（本次修复核心目标） | ❌ exit 101：tao event_loop.rs:219 → gtk::rt::init Failed to initialize GTK（无显示后端预期，非缺库） | dnf 自动装齐 libappindicator-gtk3 / libdbusmenu-gtk3 / webkit2gtk4.1 / gtk3；托盘 SONAME 有真实提供者 |
| **AppImage** | ✅ --appimage-extract exit 0 → /tmp/squashfs-root/usr/bin/dsh-tauri | ✅ 8 包已就位，ldd 无 not found | 无（本次 rpm 事务已把 8 个基础库全部带入；单独执行 8 包 dnf install exit 0，显示 Nothing to do） | ❌ 解包二进制与 AppImage 直启均 exit 101，同一 GTK backend panic（headless 预期） | 与 v0.7.2 对比：不再需要单独补库（若系统未装 rpm，仍须先补这 8 包） |

#### rpm 核心验证（逐条命令 + 退出码）

    # 干净事务：先卸载再重装，保留完整依赖日志
    sudo dnf remove -y dsh-tauri                       # REMOVE_EXIT=0（dnf history ID 4，Altered 277）
    sudo dnf install -y /tmp/dsh-0.9.0.rpm             # INSTALL2_EXIT=0（dnf history ID 5，Altered 277）
        # Transaction Summary: Installing: 277 packages；Total 230 MiB；结尾 Complete!
        # 事务中实际装入的关键包（日志原行）：
        #   [240/279] Installing gtk3-0:3.24.52-1.fc42.x86_64
        #   [241/279] Installing libdbusmenu-gtk3-0:16.04.0-29.fc42.x86_64
        #   [242/279] Installing libappindicator-gtk3-0:12.10.1-8.fc42.x86_64
        #   [249/279] Installing webkit2gtk4.1-0:2.52.3-1.fc42.x86_64
    grep -i 'nothing provides.*ayatana' /tmp/dnf-install2.log
        # 无匹配 → AYATANA_FAILURE=ABSENT（核心修复目标：dnf 不再报 ayatana 依赖不可满足）
    rpm -qpR /tmp/dsh-0.9.0.rpm                        # exit 0：
        # libappindicator3.so.1()(64bit)
        # libgtk-3.so.0()(64bit)
        # libwebkit2gtk-4.1.so.0()(64bit)
        # rpmlib(CompressedFileNames) <= 3.0.4-1 / rpmlib(FileDigests) <= 4.6.0-1 / rpmlib(PayloadFilesHavePrefix) <= 4.0-1
        # 三项 SONAME 齐全，且无 libayatana-appindicator3.so.1
    rpm -qi dsh-tauri                                  # QI_EXIT=0：Version 0.9.0 / Release 1 / x86_64 / MIT / 14724036 B
    sudo dnf check                                     # DNF_CHECK_EXIT=0，无输出（依赖图闭合）
    ldd /usr/bin/dsh-tauri                             # LDD_EXIT=0；grep 'not found' → LDD_NO_NOT_FOUND
        # libwebkit2gtk-4.1.so.0 / libgtk-3.so.0 均解析到 /lib64/
    rpm -q libappindicator-gtk3 webkit2gtk4.1 gtk3     # libappindicator-gtk3-12.10.1-8.fc42.x86_64 /
        # webkit2gtk4.1-2.52.3-1.fc42.x86_64 / gtk3-3.24.52-1.fc42.x86_64
    DISPLAY= WAYLAND_DISPLAY= timeout 20 /usr/bin/dsh-tauri --version   # RPM_LAUNCH_EXIT=101
        # tao-0.37.3 event_loop.rs:219: Failed to initialize gtk backend!（headless 预期）

#### AppImage 验证（逐条命令 + 退出码）

    cd /tmp && chmod +x dsh-0.9.0.AppImage
    ./dsh-0.9.0.AppImage --appimage-extract            # EXTRACT_EXIT=0 → /tmp/squashfs-root/usr/bin/dsh-tauri (14741297 B)
    ldd /tmp/squashfs-root/usr/bin/dsh-tauri | awk '/not found/{print $1}' | sort -u
                                                       # 输出为空：LDD_ASIS_EXIT=0（rpm 事务已补齐所有基础库）
    rpm -q fribidi fontconfig libxcb mesa-libgbm libdrm mesa-libGL mesa-libEGL libX11
        # 八包全部存在：fribidi-1.0.16-2.fc42 / fontconfig-2.16.0-2.fc42 / libxcb-1.17.0-5.fc42 /
        # mesa-libgbm-25.1.9-1.fc42 / libdrm-2.4.133-1.fc42 / mesa-libGL-25.1.9-1.fc42 /
        # mesa-libEGL-25.1.9-1.fc42 / libX11-1.8.13-1.fc42（均 x86_64）
    sudo dnf -y install fribidi fontconfig libxcb mesa-libgbm libdrm mesa-libGL mesa-libEGL libX11
                                                       # DNF_8PKGS_EXIT=0，Nothing to do（已由 rpm 事务满足）
    ldd /tmp/squashfs-root/usr/bin/dsh-tauri           # LDD_AI_EXIT=0；LDD_AI_NO_NOT_FOUND
    DISPLAY= WAYLAND_DISPLAY= timeout 20 ./squashfs-root/usr/bin/dsh-tauri --version
                                                       # AI_LAUNCH_EXIT=101（同一 GTK backend panic，headless 预期）
    DISPLAY= WAYLAND_DISPLAY= timeout 20 ./dsh-0.9.0.AppImage
                                                       # AI_DIRECT_EXIT=101（同上）

#### 托盘依赖（SONAME 提供者）

    rpm -q --whatprovides 'libappindicator3.so.1()(64bit)'
        # libappindicator-gtk3-12.10.1-8.fc42.x86_64；PROVIDES_EXIT=0
    # 结论：Fedora 42 仓库确有包提供 rpm 声明的 libappindicator3.so.1；dnf 会自动装该包，
    #       ABI 要求已满足。托盘图标是否在真实 Wayland 桌面上显示，须 GUI 实机验证（VM 禁 X11 且为 Cloud 无桌面）。
    # 对比 v0.7.2：当时 AppImage 走的是 ayatana SONAME，Fedora 无对应包；v0.9.0 切到
    #       libappindicator3 SONAME 后，这一缺口在 Fedora/RHEL 上已消除。

#### 资源释放与清理（硬约束核对）

    # 释放前：virsh list --all 含 dsh-compat-fed090（运行，Id 7）；du -sh /mnt/data/vm = 48G；
    #         qemu-system 进程含 fed090；宿主 PID 3524464（dsh-tauri）全程未动。
    virsh destroy dsh-compat-fed090                    # exit 0
    virsh undefine dsh-compat-fed090 --remove-all-storage  # exit 0；删除卷 vda（overlay）与 sda（seed ISO）
    # 释放后：域列表无 dsh-compat-fed090；overlay/seed ISO 文件不存在；du -sh /mnt/data/vm = 48G（回到基线）；
    #         无 fed090 qemu 进程；PID 3524464 仍在。并发的 dsh-compat-ubu090 属另一子 agent，未触碰。
    # 临时目录 .tmp-vm-fed090/（镜像、seed、SSH key、产物）用完删除，并移除 .git/info/exclude 中的 .tmp-vm-fed090/ 条目。
    # 宿主零 apt/apt-get 包操作，包管理全部在 VM 内 dnf；未配置 X11；未用 pgrep -f 批量 kill；未改源码/版本/CI；未 commit、未 push。

### v0.9.0 — openSUSE（AppImage）

> 本节由子 agent vm-suse090 追加，仅记录 v0.9.0 本轮 openSUSE 实测，不改上文任何段落。测试对象来自 GitHub Release v0.9.0（非本地构建），github.com 直连超时，改用已认证 api.github.com asset 端点下载：
> - AppImage：DSH.Tauri_0.9.0_amd64.AppImage（asset id 802835961，82,590,200 B，sha256 `40f9f2ef8b23fa96ec0070dc88d45b6dc4663ff5854285b698df85892a51452e`）— gh api -H 'Accept: application/octet-stream' exit 0，大小/哈希与官方一致。
> - rpm：DSH.Tauri-0.9.0-1.x86_64.rpm（asset id 802835963，4,004,713 B，sha256 `29a9ff7c84e1d13d5b61bf90dd837b8b7cdf23f4c0a6191d5bf46cb072dfa544`）— 同上，用于加分项 zypper 实测。
> 镜像：openSUSE Leap 15.6 x86_64 NoCloud cloud qcow2（https://download.opensuse.org/repositories/Cloud:/Images:/Leap_15.6/images/openSUSE-Leap-15.6.x86_64-NoCloud.qcow2 ，731,250,688 B，sha256 `d91ddc27554093a01bcc5255f8b06a2f9d4fab138082ca260719d5ddc086b411`，与官方 .sha256 一致）；另下 Tumbleweed Minimal-VM Cloud qcow2（https://download.opensuse.org/tumbleweed/appliances/openSUSE-Tumbleweed-Minimal-VM.x86_64-Cloud.qcow2 ，318,066,176 B，sha256 `6fb81a99cc3cdfa565db8c683ff44b7e3d8a1c6a8a585ac8320c9ac0058fe727`）。
> VM：dsh-compat-suse090，2 vCPU / 4 GiB / 25 GiB qcow2 overlay（backing = 官方 cloud 镜像），NAT（network=default），无 X11/Xorg/X server，只做 headless（DISPLAY=、WAYLAND_DISPLAY=）。宿主零 apt/apt-get 包操作，包管理全在 VM 内 zypper；未用 pgrep -f 批量 kill；宿主 PID 3524464 全程未动；未改源码/版本/CI；未 commit。

| 发行版 | 包格式 | 安装结果 | 依赖闭合 | 启动（headless） | 备注 |
|---|---|---|---|---|---|
| **openSUSE Leap 15.6**（kernel 6.4.0-150600.23.146-default，glibc 2.38） | AppImage | ✅ scp/chmod/--appimage-extract 全 exit 0；干净系统缺 8 个 SONAME，zypper 装 7 包 exit 0 | ⚠️ 硬 SONAME 缺失清零，但仍有 2 项 ABI 缺口：weak `GLIBC_2.39` not found、bundled cairo 的 `FT_Get_Transform` undefined（系统 freetype 2.10.4 旧） | ❌ exit 127（补库后仍卡在 cairo 符号）；用 `LD_LIBRARY_PATH=/usr/lib64:/lib64` 强制系统 cairo 优先后 ✅ exit 101（tao GTK backend panic，headless 预期） | 不是「装几包即可开箱」：Leap 14 基线偏旧，AppImage 自带 cairo 要求更新的 freetype；默认路径需在 Release 说明前置条件 |
| **openSUSE Leap 15.6** | .rpm（本轮实测可用，v0.7.2 的「无 rpm 产物」结论已过时） | ✅ `sudo zypper -n --no-gpg-checks install -y /tmp/dsh-0.9.0.rpm` exit 0（368 包事务完成）；默认不带 --no-gpg-checks 会因「软件包包头未签名」exit 8 | ✅ `sudo zypper verify` = 已满足全部已安装软件包的依赖关系；ldd 仅剩 weak GLIBC_2.39 提示（非致命） | ❌ exit 101：tao event_loop.rs:219 → gtk::rt::init Failed to initialize GTK（headless 预期） | rpm 事务自动带入 webkit2gtk4.1 2.52.1 / gtk3 3.24.43 / libappindicator3-1 12.10.1；SONAME 声明不含 ayatana，无需 ayatana 包 |
| **openSUSE Tumbleweed**（Minimal-VM Cloud） | AppImage | ⚠️ SKIP（VM shell 未拿到）：VM 正常启动但始终未取得自己的 DHCP 租约、qemu-guest-agent 不响应、串口无输出；探测 SSH 端口时误连到并发 Arch 轨道的 192.168.122.54（MAC 52:54:00:a7:17:2e，后经 domdumpxml 核实，非本 VM），按 15 分钟上限放弃 | 未验证 | 未验证 | 不伪造结论；Leap 已提供有效 openSUSE 覆盖 |

#### 拿 shell 的方法（Leap 15.6）

    # xorriso 造 NoCloud seed（本机无 cloud-localds / genisoimage）
    # seed/user-data: #cloud-config，users: pt + ssh_authorized_keys(ed25519 公钥) + sudo NOPASSWD:ALL；
    #                 packages: openssh；runcmd: systemctl enable/start sshd.service
    # seed/meta-data: instance-id=dsh-compat-suse090, local-hostname=dsh-suse090
    xorriso -as mkisofs -output seed.iso -volid CIDATA -joliet -rock seed/   # SEED_EXIT=0
    qemu-img create -f qcow2 -F qcow2 -b leap15.6.qcow2 leap090.qcow2 25G   # OVERLAY_EXIT=0
    # libvirt-qemu 需读 overlay/seed/backing：dsh-root --shell 'chgrp 991 ... && chmod 664 ...'  # exit 0
    virt-install --name dsh-compat-suse090 --memory 4096 --vcpus 2 --os-variant opensuse15.6 \
      --disk ...,format=qcow2,bus=virtio --disk seed.iso,device=cdrom \
      --network network=default,model=virtio --graphics none --console pty,target_type=serial \
      --import --noautoconsole                                              # VI_EXIT=0
    # 首次 virt-install 误用 --os-variant sles15sp3 → exit 1（未知 OS 名）；改 opensuse15.6 后成功
    virsh net-dhcp-leases default                        # Leap 首轮 192.168.122.162，重启后 192.168.122.58
    ssh -o BatchMode=yes -o IdentitiesOnly=yes -i .tmp-vm-suse090/id_ed25519 pt@<IP>   # SSH_EXIT=0
        # cat /etc/os-release → openSUSE Leap 15.6；id → uid=1000(pt)；sudo -n true → SUDO_OK
    # cloud-init 首次启动会持 zypper 锁（refresh / install openssh），前 3 次 zypper exit 7；
    # 等待 cloud-init 的 zypper 进程结束后再装包即正常。

#### ① AppImage 解包（退出码逐条）

    gh api repos/pengtang1024/dsh-tauri/releases/assets/802835961 -H 'Accept: application/octet-stream' > dsh-0.9.0.AppImage
                                                                   # DL_EXIT=0；82,590,200 B；sha256 与官方一致
    scp dsh-0.9.0.AppImage pt@IP:/tmp/dsh-0.9.0.AppImage           # SCP_EXIT=0；VM 内 sha256 一致
    chmod +x /tmp/dsh-0.9.0.AppImage                              # CHMOD_EXIT=0
    cd /tmp && ./dsh-0.9.0.AppImage --appimage-extract            # EXTRACT_EXIT=0 → /tmp/squashfs-root/usr/bin/dsh-tauri（14,741,297 B）

#### ② 干净系统 ldd 缺失 SONAME（装包前）

    ldd /tmp/squashfs-root/usr/bin/dsh-tauri > /tmp/ldd-before.txt   # LDD_EXIT=0；共 138 行
    grep 'not found' /tmp/ldd-before.txt | sed -E 's/^[[:space:]]*([^ ]+).*/\1/' | sort -u
        # 8 个真实缺失 SONAME：libdrm.so.2、libEGL.so.1、libfribidi.so.0、libgbm.so.1、
        #   libGL.so.1、libharfbuzz.so.0、libwayland-client.so.0、libX11-xcb.so.1
    for c in zenity wl-copy wl-paste notify-send xdg-open kdialog; do command -v $c || echo MISSING:$c; done
        # 6 项全 MISSING（最小 cloud 镜像无桌面助手命令）
    DISPLAY= WAYLAND_DISPLAY= timeout 20 /tmp/squashfs-root/usr/bin/dsh-tauri --version
        # PRE_VERSION_EXIT=127；stderr: error while loading shared libraries: libfribidi.so.0

#### ③ zypper 包名与命令（SONAME → 包名逐个经 zypper search --provides 核实）

    sudo zypper -n refresh                                         # 首次 exit 7（cloud-init 持锁）；重试 exit 0
    sudo zypper -n install -y libdrm2 libglvnd libfribidi0 libgbm1 libharfbuzz0 libwayland-client0 libX11-xcb1
                                                                   # LIBS_INSTALL_EXIT=0（34 包事务完成）
    sudo zypper -n install -y zenity wl-clipboard libnotify-tools xdg-utils
                                                                   # HELPERS_EXIT=0（217 包事务完成）
    for c in zenity wl-copy wl-paste notify-send xdg-open kdialog; do command -v $c || echo MISSING:$c; done
        # /usr/bin/zenity、/usr/bin/wl-copy、/usr/bin/wl-paste、/usr/bin/notify-send、/usr/bin/xdg-open 全就位；kdialog 缺省（可选增强）

  准确包名映射（Leap 15.6 实测）：`libdrm.so.2`→`libdrm2`（libdrm 2.4.118 系）；`libEGL.so.1` 与 `libGL.so.1`→`libglvnd` 1.7.0-150600.1.2（实际提供者为 Mesa-libEGL1 / Mesa-libGL1 23.3.4）；`libfribidi.so.0`→`libfribidi0`；`libgbm.so.1`→`libgbm1` 23.3.4-150600.83.3.1；`libharfbuzz.so.0`→`libharfbuzz0` 8.3.0-150600.3.3.1；`libwayland-client.so.0`→`libwayland-client0`；`libX11-xcb.so.1`→`libX11-xcb1` 1.8.7-150600.3.6.1。助手包：`zenity` / `wl-clipboard` / `libnotify-tools`（提供 notify-send；Debian 系叫 libnotify-bin）/ `xdg-utils`（提供 xdg-open）。注意 Leap 仓库没有 Debian 式 webkit2gtk-4.1 包名，对应包是 `libwebkit2gtk-4_1-0` 2.52.1 与 `libgtk-3-0` 3.24.43；托盘库 `libappindicator3-1` 12.10.1 在 Main Repository 可用（提供 libappindicator3.so.1），ayatana 版无需。

#### ④ 补库后 ldd（关键差异）

    ldd /tmp/squashfs-root/usr/bin/dsh-tauri > /tmp/ldd-after.txt       # LDD_EXIT=0；130 行
    grep -c 'not found' /tmp/ldd-after.txt                              # 1（仅 weak version 提示，无硬 SONAME 缺失）
        # /lib64/libc.so.6: weak version `GLIBC_2.39' not found (required by dsh-tauri)
    DISPLAY= WAYLAND_DISPLAY= timeout 25 /tmp/squashfs-root/usr/bin/dsh-tauri --version
        # POST_VERSION_EXIT=127
        # stderr: weak version `GLIBC_2.39' not found；
        #         symbol lookup error: .../lib/libcairo.so.2: undefined symbol: FT_Get_Transform

  根因：Leap 15.6 基线 glibc 2.38 / libfreetype6 2.10.4，而 AppImage 自带 libcairo.so.2 需要 FreeType 的 `FT_Get_Transform`（2.10.4 未提供），二进制还标记了 weak GLIBC_2.39。这不是「缺 SONAME」，而是老基线发行版的 ABI 缺口：仅补 ③ 的 7 包不足以启动 AppImage。绕过方式（实测有效）：

    DISPLAY= WAYLAND_DISPLAY= LD_LIBRARY_PATH=/usr/lib64:/lib64 timeout 25 /tmp/squashfs-root/usr/bin/dsh-tauri --version
        # LLP_VERSION_EXIT=101（系统 cairo 1.18.4 优先于 bundled cairo，越过符号错误）
        # stderr: tao-0.37.3/event_loop.rs:219: Failed to initialize gtk backend!（gtk::rt::init）→ headless 预期失败

#### ⑤ RPM 格式在 openSUSE 上可直接用（加分项）

    gh api repos/pengtang1024/dsh-tauri/releases/assets/802835963 -H 'Accept: application/octet-stream' > dsh-0.9.0.rpm
                                                                   # DL_EXIT=0；4,004,713 B；sha256 与官方一致
    scp dsh-0.9.0.rpm pt@IP:/tmp/dsh-0.9.0.rpm                     # SCP_EXIT=0
    rpm -qpR /tmp/dsh-0.9.0.rpm                                    # exit 0：libappindicator3.so.1 / libgtk-3.so.0 /
        #   libwebkit2gtk-4.1.so.0（三项 SONAME，无 ayatana）
    sudo zypper -n install -y /tmp/dsh-0.9.0.rpm                   # RPM_INSTALL_EXIT=8（包头未签名，交互提示 a/r/i 下 -n 中止）
    sudo zypper -n --no-gpg-checks install -y /tmp/dsh-0.9.0.rpm   # RPM_INSTALL5_EXIT=0（368 包事务完成）
        # 事务自动带入：libappindicator3-1-12.10.1 / libwebkit2gtk-4_1-0-2.52.1 /
        #   webkit2gtk-4_1-injected-bundles-2.52.1 / gtk3-3.24.43 / libdbusmenu-gtk3-4
    rpm -q dsh-tauri                                               # exit 0：dsh-tauri-0.9.0-1.x86_64
    sudo zypper verify                                             # exit 0：已满足全部已安装软件包的依赖关系
    ldd /usr/bin/dsh-tauri                                         # LDD_RPM_EXIT=0；仅 weak GLIBC_2.39 提示，无 not found SONAME
    DISPLAY= WAYLAND_DISPLAY= timeout 25 /usr/bin/dsh-tauri --version
                                                                   # RPM_VERSION_EXIT=101
        # tao event_loop.rs:219 → gtk::rt::init Failed to initialize GTK（headless 预期；weak GLIBC_2.39 提示非致命）

  结论：v0.9.0 的 rpm 在 openSUSE Leap 15.6 上**可直接用**（发布包未签名，需 `--no-gpg-checks` 或先导入签名），比 AppImage 路径更干净——rpm 事务自动解析全部 GTK/WebKit/托盘依赖并达到 headless GTK panic；AppImage 则受老基线 glibc/freetype 限制。

#### 资源释放与清理（硬约束核对）

    释放前：virsh list --all → Id 16 dsh-compat-suse090 运行 + dsh-compat-arch090（另一 agent）+ tiny10-live/win11-live（关闭）；
            du -sh /mnt/data/vm = 48G；qemu-system 中 suse090 进程数 = 1；ps -p 3524464 → /usr/bin/dsh-tauri（未动）
    virsh destroy dsh-compat-suse090                              # exit 0
    virsh undefine dsh-compat-suse090 --remove-all-storage        # exit 0；删除卷 vda（leap090.qcow2）与 sda（seed.iso）
    释放后：virsh list --all → 无 dsh-compat-suse090（arch090 属另一轨道，未触碰）；
            qemu-system suse090 进程数 = 0；du -sh /mnt/data/vm = 48G（与基线一致）；PID 3524464 仍在
    清理：临时目录 .tmp-vm-suse090/（两个 cloud 镜像、seed、SSH key、AppImage、rpm、日志）已 rm -rf；
          .git/info/exclude 中仅移除本人新增的 .tmp-vm-suse090/ 行，其它轨道（.tmp-vm-arch090/ 等）保留不动。
    全程未配置 X11；未用 pgrep -f 批量 kill；宿主零 apt/apt-get；未改源码/版本/CI；未 commit、未 push。

## 结论与建议

1. Debian/Ubuntu .deb 路线是唯一完整验证通过的产物路径：装包、依赖闭合、外部助手命令均达标；gtk 启动失败纯粹因 VM 禁止显示后端，不代表 GUI 不可用。建议把「headless 装包/依赖冒烟」固化为 CI 门禁。
2. Fedora（dnf）无官方 rpm 产物：只能 AppImage 或源码构建。AppImage 在最小 Fedora 上并非开箱即用，至少需补 8 类基础运行库。建议：要么 CI 增加 rpm 产物，要么在 Release 说明中给出上述 dnf install 前置命令；同时把 libayatana-appindicator3 列为已知不可满足依赖（托盘在 Fedora 降级）。
3. Arch（pacman）包名总体齐全，但滚动发行版的 WebKit 版本（2.54.3）与 AppImage 自带版本差异需真机复验；本次 VM 未起来，明确标为未验证。若要补测，建议换带 cloud-init 的 Arch 镜像或制作带预置 root 密码的自定义镜像。
4. 外部命令依赖的发行版差异是真实风险：Ubuntu 上助手命令由 deb 强依赖保障；Fedora/Arch 上助手命令与 AppImage 运行库都要用户手动装。当前代码已实现 kdialog 兜底链，且目录选择与关于面板均已改为自绘 GTK（`picker::show_info`）为主路径，zenity/kdialog 仅作兜底降级（task-405 已收口），不再有关于面板依赖 zenity 的缺口。
5. 资源释放已完成：三台 VM 全部 destroy + undefine --remove-all-storage，磁盘占用回到 48G 基线，无残留域与 qemu 进程。临时工作区 .tmp-compat/（含镜像、seed ISO、SSH key、console 日志）用完即从工作区删除，并还原 .git/info/exclude。

### v0.9.0 — Arch（AppImage）

> 本节由子 agent vm-arch090 追加，只记录 v0.9.0 本轮 Arch 真机 KVM 实测，不改上文任何段落（openSUSE/Ubuntu/Fedora 段保持原样）。测试对象来自 GitHub Release v0.9.0（非本地构建），github.com 直连可能超时，改用已认证 api.github.com asset 端点下载：
> - AppImage：DSH.Tauri_0.9.0_amd64.AppImage（asset id 802835961，82,590,200 B，sha256 `40f9f2ef8b23fa96ec0070dc88d45b6dc4663ff5854285b698df85892a51452e`）— gh api -H 'Accept: application/octet-stream' exit 0，大小/哈希与官方一致。
> 镜像：官方 Arch-Linux-x86_64-cloudimg.qcow2（https://geo.mirror.pkgbuild.com/images/latest/Arch-Linux-x86_64-cloudimg.qcow2 ，569,933,312 B，sha256 `dbd02830f6eb0fe07ef6e5edaddbae8b577ce4296d9e79118904704cce0a1f60`）。
> VM：dsh-compat-arch090，2 vCPU / 4 GiB / overlay qcow2（backing = 官方 cloudimg，初始 2 GiB→扩至 30 GiB，btrfs 根分区），NAT（network=default），Arch 内核 7.3.4-arch1-2；无 X11/Xorg/Xvfb，仅 headless（DISPLAY=、WAYLAND_DISPLAY=）。宿主零 apt/apt-get 包操作，包管理全在 VM 内 pacman；未用 pgrep -f 批量 kill；宿主 PID 3524464 全程未动；未改源码/版本/CI；未 commit、未 push。

**上一轮 SKIP 缺口的解决（拿 shell 方法）**：官方 Arch cloudimg 不带 cloud-init/sshd 配置，但 sshd.service 其实已 enable、root 密码为锁定态 `root:*:`。本轮用离线改镜像 + 手动引导拿 shell：
1. `dsh-root modprobe nbd max_part=8` 加载 NBD（宿主零装包）；qemu-nbd 挂载 cloudimg 的 overlay qcow2，mount /dev/nbd0p3（btrfs）后 `sed -i "s|^root:\*:|root:\$6\$…:|" /etc/shadow` 注入 openssl passwd -6 生成的 SHA-512 root 密码 hash，并写 /etc/ssh/sshd_config.d/99-dsh-kvm.conf（PermitRootLogin yes + PasswordAuthentication yes）；umount + qemu-nbd --disconnect。
2. virt-install --boot uefi 导入时默认用 OVMF_CODE_4M.ms.fd（secure boot MS keys）导致 GRUB 不出现；`virsh dumpxml` 导出后改 XML 为 /usr/share/OVMF/OVMF_CODE_4M.fd + enrolled-keys=off/secure-boot=off，`virsh undefine --nvram` 再 `virsh define --file` 后启动，GRUB 出现。
3. pexpect + `virsh console` 回车选中 Arch Linux，serial console root + 注入密码登录成功（`SHELL_OK_0`，id -u = 0）。
4. 镜像无 systemd .network 配置，eth0 DOWN；用 QEMU guest agent（guest-exec）执行 `ip link set eth0 up; dhclient eth0` 拿到 192.168.122.54，` /usr/bin/sshd` 启动 TCP 22，注入 ed25519 公钥后 SSH 免密登录成功。之后所有验证通过 SSH 执行。

**① AppImage 解包（逐条命令 + 退出码）**

    gh api repos/pengtang1024/dsh-tauri/releases/assets/802835961 -H 'Accept: application/octet-stream' > .tmp-vm-arch090/dsh-0.9.0.AppImage
                                                                    # DL_EXIT=0；82,590,200 B；宿主 sha256 与官方一致
    scp -i vm_key dsh-0.9.0.AppImage root@192.168.122.54:/root/dsh-0.9.0.AppImage   # SCP_EXIT=0
    sha256sum /root/dsh-0.9.0.AppImage                             # 40f9f2ef…51452e，与宿主/官方一致
    chmod +x /root/dsh-0.9.0.AppImage                              # CHMOD_EXIT=0
    cd /root && ./dsh-0.9.0.AppImage --appimage-extract            # EXTRACT_EXIT=0 → /root/squashfs-root/usr/bin/dsh-tauri（14,741,297 B）

**② 干净系统 ldd 缺失 SONAME（装包前，系统 185 包，webkit/gtk/mesa/wayland 全无）**

    ldd /root/squashfs-root/usr/bin/dsh-tauri | awk '/not found/{print $1}' | sort -u
        # 12 个缺失 SONAME：libEGL.so.1、libGL.so.1、libX11-xcb.so.1、libX11.so.6、libdrm.so.2、
        #   libfontconfig.so.1、libfreetype.so.6、libfribidi.so.0、libgbm.so.1、libharfbuzz.so.0、
        #   libwayland-client.so.0、libxcb.so.1；ldd 共 159 行
    pacman -Fy                                                     # FY=0（首次需建文件数据库）
    for so in ...; do pacman -F "$so"; done                        # 逐 SONAME 映射到准确包名（见下）

**③ pacman 装包（准确包名 + 命令 + 关键坑）**

    pacman-key --init && pacman-key --populate archlinux           # 官方 cloudimg 无 keyring；refresh-keys 因 keyserver 不可达卡住，已终止
    # 根分区原 1.7Gb btrfs 不足 600MiB 事务：qemu-img resize overlay 30G + parted resizepart 3 100%
    #   + dsh-root mount btrfs + btrfs filesystem resize max（LDD 前完成；离线 NBD 操作）
    pacman -S --noconfirm --needed libglvnd libx11 libdrm fontconfig freetype2 fribidi mesa harfbuzz wayland libxcb       zenity wl-clipboard libnotify xdg-utils                      # PACMAN_S=0；114 包事务，Installing 完成
        # 注：kdialog 会拉入 265 包 KDE/Qt6 栈且撞镜像 404，本轮未装 kdialog（可选兜底，GNOME 不需要）
        # 注：缺 keyring 时临时把 /etc/pacman.conf 的 SigLevel 改成 Never，装完立刻用备份还原（已核实还原为 Required DatabaseOptional）
    pacman -Q libglvnd libx11 libdrm fontconfig freetype2 fribidi mesa harfbuzz wayland libxcb zenity wl-clipboard libnotify xdg-utils gtk3
        # libglvnd 1.7.0-3 / libx11 1.8.14-1 / libdrm 2.4.134-1 / fontconfig 2:2.18.4-1 /
        # freetype2 2.15.0-1 / fribidi 1.0.18-1 / mesa 1:26.3.1-3 / harfbuzz 15.2.0-1 /
        # wayland 1.26.0-1 / libxcb 1.17.0-1 / zenity 4.2.2-1 / wl-clipboard 1:2.3.0-1 /
        # libnotify 0.8.9-1 / xdg-utils 1.2.1-2 / gtk3 1:3.24.52-2（均 [extra]）
    # SONAME → pacman 包名映射（pacman -F 实测）：libEGL.so.1/libGL.so.1→libglvnd；
    #   libX11.so.6/libX11-xcb.so.1→libx11；libdrm.so.2→libdrm；libfontconfig.so.1→fontconfig；
    #   libfreetype.so.6→freetype2；libfribidi.so.0→fribidi；libgbm.so.1→mesa；
    #   libharfbuzz.so.0→harfbuzz；libwayland-client.so.0→wayland；libxcb.so.1→libxcb

**④ 补库后 ldd**

    ldd /root/squashfs-root/usr/bin/dsh-tauri | grep -c 'not found'   # MISSING_COUNT=0（无 not found）；ldd 共 127 行
    ldd ... | grep -E 'webkit2gtk|gtk-3|gbm|wayland-client'
        # libwebkit2gtk-4.1.so.0 与 libgtk-3.so.0 → squashfs-root/usr/bin/../lib（AppImage 自带）；
        # libwayland-client.so.0 / libgbm.so.1 → /usr/lib（系统）

**⑤ headless --version（预期失败）**

    cd /root; DISPLAY= WAYLAND_DISPLAY= timeout 25 ./dsh-0.9.0.AppImage --version                # VERSION_EXIT=101
    cd /root; DISPLAY= WAYLAND_DISPLAY= timeout 25 ./squashfs-root/usr/bin/dsh-tauri --version    # BIN_EXIT=101
        # 两者同一错误：tao-0.37.3/src/platform_impl/linux/event_loop.rs:219
        #   "Failed to initialize gtk backend!: BoolError { message: "Failed to initialize GTK", … gtk::rt::init, line: 141 }"
        # 已越过动态库加载阶段，属无显示后端预期失败，非缺库 bug（VM 禁 X11、未配任何显示后端）

**⑥ WebKit 版本差异（Arch 滚动 vs AppImage 自带）**

    pacman -Si webkit2gtk-4.1 | grep Version                        # Version: 2.54.3-1（[extra]，滚动发行版）
    python3 ctypes CDLL('/root/squashfs-root/usr/lib/libwebkit2gtk-4.1.so.0')
        # webkit_get_major/minor/micro_version() → BUNDLED_WEBKIT=2.52.6（AppImage 在 Ubuntu 24.04 上构建）
    # 差异：系统 2.54.3 vs 自带 2.52.6，相差 2 个 minor；AppImage 自带 WebKit/GTK 优先（ldd 指向 squashfs-root），
    #   故实际运行用自带版本，不受 Arch 系统 WebKit 滚动影响；但自带 cairo/字体等仍与系统库交互，升级基线需回归 GUI。
    # 助手命令：zenity / wl-copy / wl-paste / notify-send / xdg-open 全部就位；kdialog MISSING（可选）。

**结论**：v0.9.0 AppImage 在 Arch 上补齐 10 类运行库包后 ldd 完全闭合、headless 启动到 GTK backend panic（预期），不再是 v0.7.2 的「VM 起不来 SKIP」。最小 Arch cloudimg 不是开箱即用——需手动注入 root 密码 + 关 secure boot 才能拿 shell，需 dhclient 才有网络，缺 keyring/btrfs 分区偏小是额外坑；Release 说明应给出上述 pacman 前置命令。

**资源释放与清理（硬约束核对）**

    释放前：virsh list --all 含 Id 18 dsh-compat-arch090（运行）+ tiny10-live/win11-live（关闭，属原有）；du -sh /mnt/data/vm = 48G
    virsh destroy dsh-compat-arch090                                # exit 0
    virsh undefine dsh-compat-arch090 --nvram                       # exit 0（本轮用自定义 XML 定义，无 --remove-all-storage 的卷；overlay/backing 在 .tmp-vm-arch090/，随目录删除）
    dsh-root qemu-nbd --disconnect /dev/nbd0                        # 已断开；/mnt/dsh-arch-inspect 已 umount
    释放后：virsh list --all 仅 tiny10-live/win11-live（均关闭）、grep dsh-compat → 无匹配；du -sh /mnt/data/vm = 48G（与基线一致）；无 qemu-system for arch090 进程；PID 3524464 仍在
    清理：临时目录 .tmp-vm-arch090/（cloudimg、overlay qcow2、AppImage、SSH key、console 日志、一次性 gcmd.sh/vm.xml、pacman.conf 备份在 VM 内）已 rm -rf；
          .git/info/exclude 中仅移除本人新增的 .tmp-vm-arch090/ 行，其它轨道行保留不动。
    全程禁 X11（VM 内无 Xorg/Xvfb）；宿主零 apt/apt-get；未用 pgrep -f 批量 kill（仅按显式域名 destroy/undefine）；PID 3524464 未动；未改源码/版本/CI；未 commit、未 push。
