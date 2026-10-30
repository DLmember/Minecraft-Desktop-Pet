# Minecraft桌宠 · Linux 版

![Linux](https://img.shields.io/badge/Linux-x86__64%20%7C%20aarch64%20%7C%20i686-FCC624?logo=linux&logoColor=black)
![X11](https://img.shields.io/badge/X11-XWayland-6A5ACD)
![GTK3](https://img.shields.io/badge/GTK3-3.24%2B-4A90D9)

**Linux 分支由 Bilibili [灰白happygray](https://space.bilibili.com/3690971449526321) 开发维护**（本目录 `linux/` 的全部实现、X11 / Wayland 适配、deb 与 Arch 打包）。
上游项目的 Windows / macOS 部分由 GPT 6 Astra 与 DLmember 完成，这里只复用 `native/` 的渲染器。

Linux 前端复用 `native/` 的原版 Rust 渲染器（同一套皮肤、动画与设置 JSON），
本目录只负责窗口、输入、托盘与控制台。

> 独立粉丝作品，与 Mojang Studios / Microsoft 无关联。

## 功能

- **透明置顶窗口**：X11 32 位 ARGB，跳过任务栏；可拖动，位置会保存
- **完全鼠标穿透**：Shape 输入区域置空，指针移到桌宠可见部分时整体淡至 25%
- **鼠标全方位跟随**：头部朝向跟着光标走，不限于桌宠附近（Wayland 下也能做到，见下文）
- **全局键鼠互动**：打字动画、Shift 潜行、点击挥臂（XRecord + 可选 evdev，只读、不抢焦点）
- **系统托盘**：StatusNotifierItem，左键打开控制台，右键快捷菜单
- **GTK3 控制台**：基本 / 输入与聊天 / 视角 / 自定义姿态 / 皮肤与关于 / 启动与性能
- **皮肤**：本地 PNG 导入，或按 Java 版用户名 / UUID 获取正版皮肤与披风
- **登录自启**：写入 `~/.config/autostart`，支持延迟启动
- **单实例**：再次启动只会唤出已运行实例的控制台，不会多开

## 系统要求

- x86_64 / aarch64 / i686 Linux
- X11 会话，或 Wayland 会话中的 XWayland（`DISPLAY` 必须存在）
- 桌面合成器（KWin、Mutter、picom 等）提供 32 位 ARGB visual，否则直接报错而不是降级成黑底窗口
- 运行时需要 GTK3；`x11rb` 直接实现 X11 协议，不需要 libX11 开发头文件
- X 服务器需要 Shape 与 RECORD 扩展（Xorg / XWayland 默认提供）

## 安装

发行版安装包（`scripts/build-linux-packages.sh` 生成）：

```bash
sudo apt install ./minecraft-desktop-pet_<版本>_amd64.deb      # Debian / Ubuntu（amd64 / arm64 / i386）
sudo pacman -U minecraft-desktop-pet-<版本>-1-x86_64.pkg.tar.zst  # Arch（x86_64 / aarch64 / i686）
```

deb 的依赖写成 `libgtk-3-0 | libgtk-3-0t64`、`libglib2.0-0 | libglib2.0-0t64`，
Ubuntu 24.04 起改用 t64 包名也能装；`curl`、`xdg-utils` 为推荐项。

从源码安装到当前用户或系统：

```bash
linux/install.sh                 # ~/.local/bin 与 ~/.local/share/applications
PREFIX=/usr/local sudo -E linux/install.sh linux/target/release/minecraft-desktop-pet
```

## 运行

```bash
minecraft-desktop-pet
```

托盘可用时启动不弹控制台；托盘不可用时才自动打开控制台，避免找不到入口。

| 参数 | 说明 |
| --- | --- |
| `--console` | 启动时打开控制台 |
| `--autostart` | 登录自启模式：应用 `autostartDelay` |
| `--data-dir PATH` | 使用指定设置目录（可隔离多份配置） |
| `--import-skin PATH` | 无界面导入本地 PNG 皮肤后退出 |
| `--fetch-skin NAME\|UUID` | 无界面获取正版皮肤/披风后退出 |
| `--smoke-test` | 无 X11 环境下渲染一帧并打印设置统计 |
| `--render-png PATH` | 配合 `--smoke-test` 输出 PNG |
| `--capture WINID PATH` | 抓取指定 X11 窗口为 PNG（调试用） |
| `--probe-input SECONDS` | 打印全局输入事件与 XRecord / evdev 状态（调试用） |
| `--probe-pointer SECONDS` | 对比 KWin 上报的光标位置与 XQueryPointer（调试用） |

## 操作

| 操作 | 效果 |
| --- | --- |
| 左键单击桌宠 | 挥动主手 |
| 左键拖动桌宠 | 移动位置并保存；需先关闭“完全鼠标穿透” |
| 双击桌宠 | 打开控制台 |
| 右键桌宠 | 姿势 / 主手 / 置顶 / 穿透 / 找回 / 退出 |
| 托盘左键 | 打开控制台 |
| 托盘右键 | 快捷菜单（含“保持置顶”“完全鼠标穿透”“导入皮肤”） |
| `Ctrl+Alt+P` | 打开控制台（XGrabKey 全局热键） |
| 再次运行程序 | 唤出已运行实例的控制台 |
| 关闭控制台窗口 | 隐藏到托盘，桌宠继续运行 |

开启穿透后，指针移到桌宠可见部分时约 0.15 秒淡到 25%，移开恢复；透明空白处不触发。

## Wayland 下的两个限制与处理

XWayland 只是 Wayland 的一个 X11 客户端，所以两件事在原生 Wayland 窗口上不成立：
**全局按键**和**全局光标位置**。本版都做了处理。

### 全局光标：KWin 脚本桥接

`XQueryPointer` 只在指针位于 X11 窗口之上时更新，指针移到原生 Wayland 窗口或桌面上就会
停住，表现为“只有靠近桌宠才跟手”。程序启动时会：

1. 在 `$XDG_RUNTIME_DIR` 写一个临时 KWin 脚本，用 `org.kde.kwin.Scripting.loadScript` 加载；
2. 脚本每 33 ms 把 `workspace.cursorPos` 乘以当前输出的 `devicePixelRatio`（KWin 给的是
   逻辑坐标，X11 根窗口是物理像素）后，通过 D-Bus `org.kde.MinecraftPet.Pointer` 发回；
3. 退出时卸载脚本并删除临时文件。

拿不到数据（X11 会话、其它桌面、脚本被卸载）时自动回退到 `XQueryPointer`。
`--probe-pointer` 可以同时打印两边的坐标用于排查。

### 全局键盘：KWin 转发或 evdev

XRecord 只能看到 KWin 转发给 XWayland 的按键，而 KWin 默认只转发组合键。两种办法，可同时启用：

1. **KDE 转发**：系统设置 → 应用程序权限 → 旧版 X11 应用支持 → 允许读取“所有按键”，
   并勾选“鼠标按键”。命令行等价：

   ```bash
   kwriteconfig6 --file kwinrc --group Xwayland --key XwaylandEavesdrops All
   kwriteconfig6 --file kwinrc --group Xwayland --key XwaylandEavesdropsMouse true
   qdbus6 org.kde.KWin /KWin reconfigure
   ```

2. **evdev**：`sudo usermod -aG input $USER` 后重新登录。程序会直接读取 `/dev/input` 下的
   键盘设备，优先于 XRecord，GNOME 等其它 Wayland 桌面同样可用。

两种办法都会让更多程序读到全局按键（前者是所有 X11 程序，后者是 input 组内的所有进程），
请按需权衡。控制台诊断页会显示当前生效的来源与按键计数。

## 与 macOS / Windows 版的差异

- **hyp 快速队伍聊天不可用**：把 `#` 替换为 `/p chat ` 需要拦截并重新注入按键，
  XRecord/evdev 都只能旁路监听、无法消费事件。设置项保留但置灰，切回其它平台仍然有效。
- **音乐音符不生效**：核心渲染器没有设置 `pose.music_playing`，macOS 版同样不显示。
- **托盘图标**：走 StatusNotifierItem。GNOME 需要 AppIndicator 扩展；没有托盘宿主时启动会直接
  打开控制台，之后仍可用 `Ctrl+Alt+P` 或再次运行程序唤出。
- **进程优先级**：映射为 POSIX nice，`above` / `high` 需要 `CAP_SYS_NICE` 或 root，
  失败时提示并保持原优先级；“正常”档不调用 `setpriority`。
- **控制台与右键菜单**强制 `GDK_BACKEND=x11`，保证弹出菜单能取到全局指针位置。

## 构建

Debian / Ubuntu：

```bash
sudo apt install build-essential pkg-config libgtk-3-dev
```

Fedora：`sudo dnf install gcc pkg-config gtk3-devel`；Arch：`sudo pacman -S base-devel pkg-config gtk3`。

```bash
cargo build --release --manifest-path linux/Cargo.toml
# 或打出便携压缩包
scripts/build-linux.sh /absolute/path/to/output
```

产物：`linux/target/release/minecraft-desktop-pet`。

### 交叉编译三个架构

直接在本机构建会带上滚动发行版的新 glibc（例如 GLIBC_2.43），旧发行版跑不起来。
正式包统一用 Debian bookworm 的 sysroot（glibc 2.36）交叉编译，实测产物只要求 **GLIBC_2.34**。

```bash
rustup target add aarch64-unknown-linux-gnu i686-unknown-linux-gnu
# 生成 sysroot（自动解析 libgtk-3-dev 依赖闭包并补齐缺失的 .pc）
scripts/debian-sysroot.py amd64 /tmp/pet-sysroot/sysroot-amd64
scripts/debian-sysroot.py arm64 /tmp/pet-sysroot/sysroot-arm64
scripts/debian-sysroot.py i386  /tmp/pet-sysroot/sysroot-i386
# 交叉编译
scripts/build-linux-cross.sh /tmp/pet-sysroot
# 打 deb + Arch 包
scripts/build-linux-packages.sh
```

需要 `clang`、`lld`、`pkg-config`、`ar`、`zstd`、`fakeroot`（Arch）以及 `makepkg`。
`PET_ARCHES="amd64"` 可以只打某个架构。

| 产物 | 架构 |
| --- | --- |
| `minecraft-desktop-pet_<版本>_amd64.deb` / `_arm64.deb` / `_i386.deb` | x86_64 / arm64 / i686 |
| `minecraft-desktop-pet-<版本>-1-{x86_64,aarch64,i686}.pkg.tar.zst` | 同上 |

## 数据目录

默认 `$XDG_DATA_HOME/MinecraftDesktopPet`（通常 `~/.local/share/MinecraftDesktopPet`）：

- `settings.json` / `.bak`：全部偏好（含窗口位置、穿透、优先级）
- `head-follow.json` / `.bak`：六项头部参数
- `skin.png`、`cape.png`、`profile-skin-*.png`：皮肤与披风缓存

自启项：`~/.config/autostart/minecraft-desktop-pet.desktop`，由控制台“登录桌面后自动启动”
创建/删除，记录当前可执行文件与数据目录。

## 故障排查

- **“当前 X 服务器没有 32 位 ARGB 视觉”**：启用合成器；Wayland 会话确认 XWayland 在运行。
- **穿透后找不到入口**：`Ctrl+Alt+P`、单击托盘图标，或再次运行程序；也可用 `--console` 启动一次。
- **头部不跟手、只在桌宠附近生效**：`--probe-pointer` 看 KWin 上报是否为空。为空时检查启动日志
  里的报错；程序异常退出可能留下未被卸载的 KWin 脚本，下次启动会自动替换，
  也可以在 KDE 的“KWin 脚本”设置里禁用 `minecraft-pet-pointer`。
- **打字没有动画**：确认“全局键盘互动”开启；按上文配置 KWin 转发或 evdev；
  控制台诊断页会显示 XRecord / evdev 状态与按键计数。
- **窗口位置每次都变**：位置通过 `WM_NORMAL_HINTS` 的 `USPosition` 交给窗口管理器；
  个别窗口管理器会忽略该提示，此时拖动一次即可重新保存。
- **控制台字体/主题异常**：GTK3 跟随系统主题与字体，安装中文字体（如 Noto Sans CJK）后重启。
- **自启失效**：检查 `.desktop` 里的 `Exec` 是否为当前绝对路径，移动二进制后重新勾选一次。

## 许可

应用代码与项目原创示例皮肤采用 MIT License；依赖许可见 `THIRD_PARTY_NOTICES.md` 与
`native/licenses/`。用户导入的皮肤不受本项目许可证覆盖。
