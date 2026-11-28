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


## 结论与建议

1. Debian/Ubuntu .deb 路线是唯一完整验证通过的产物路径：装包、依赖闭合、外部助手命令均达标；gtk 启动失败纯粹因 VM 禁止显示后端，不代表 GUI 不可用。建议把「headless 装包/依赖冒烟」固化为 CI 门禁。
2. Fedora（dnf）无官方 rpm 产物：只能 AppImage 或源码构建。AppImage 在最小 Fedora 上并非开箱即用，至少需补 8 类基础运行库。建议：要么 CI 增加 rpm 产物，要么在 Release 说明中给出上述 dnf install 前置命令；同时把 libayatana-appindicator3 列为已知不可满足依赖（托盘在 Fedora 降级）。
3. Arch（pacman）包名总体齐全，但滚动发行版的 WebKit 版本（2.54.3）与 AppImage 自带版本差异需真机复验；本次 VM 未起来，明确标为未验证。若要补测，建议换带 cloud-init 的 Arch 镜像或制作带预置 root 密码的自定义镜像。
4. 外部命令依赖的发行版差异是真实风险：Ubuntu 上助手命令由 deb 强依赖保障；Fedora/Arch 上助手命令与 AppImage 运行库都要用户手动装。当前代码已实现 kdialog 兜底链，且目录选择与关于面板均已改为自绘 GTK（`picker::show_info`）为主路径，zenity/kdialog 仅作兜底降级（task-405 已收口），不再有关于面板依赖 zenity 的缺口。
5. 资源释放已完成：三台 VM 全部 destroy + undefine --remove-all-storage，磁盘占用回到 48G 基线，无残留域与 qemu 进程。临时工作区 .tmp-compat/（含镜像、seed ISO、SSH key、console 日志）用完即从工作区删除，并还原 .git/info/exclude。
