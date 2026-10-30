# Minecraft桌宠

![Windows](https://img.shields.io/badge/Windows-10%2F11%20x64-0078D6?logo=windows&logoColor=white)
![macOS](https://img.shields.io/badge/macOS-12%2B-000000?logo=apple&logoColor=white)
![Linux](https://img.shields.io/badge/Linux-x86__64%20%7C%20arm64%20%7C%20i686-FCC624?logo=linux&logoColor=black)

让一个 Minecraft 风格的小人待在桌面上，跟着鼠标转头，陪你敲键盘。

**由GPT 6 Astra制作**

**Linux 分支由 Bilibili [灰白happygray](https://space.bilibili.com/3690971449526321) 开发移植**（`linux/` 前端、X11 / Wayland 适配、deb 与 Arch 打包）。

> 本项目是独立粉丝作品，与 Mojang Studios 或 Microsoft 无关联，未获其授权、赞助或官方认可。Minecraft 名称及相关商标归其各自权利人所有。

## 下载

前往 [最新版本](https://github.com/DLmember/Minecraft-Desktop-Pet/releases/latest)，下载 **Minecraft-Desktop-Pet-2.5.1-Windows.zip**，解压后双击 **Minecraft桌宠.exe**。

| 平台 | 架构 | 状态 |
| --- | --- | --- |
| Windows 10/11 | x64 / x86 / arm64 | 主分支 |
| macOS 12+ | Apple Silicon / Intel 通用 | 主分支（2.4.12） |
| Linux | x86_64 / aarch64 / i686 | **Linux 分支**，由 [灰白happygray](https://space.bilibili.com/3690971449526321) 维护 |

支持 **Windows 10/11 x64 、 macOS 12+**；Linux（X11 / XWayland）提供源码构建版，见下文。便携运行，不需要安装 Electron、Java 或额外运行环境。v2.5.1 提供 Windows 版；macOS 尚未更新，请暂时使用 v2.4.12。

**首次启动默认开启完全鼠标穿透。** 请双击任务栏托盘图标打开控制台；想拖动桌宠时先取消“完全鼠标穿透”。图标可能收在任务栏的隐藏图标区域。Linux 版同样有托盘图标（StatusNotifierItem），穿透时也可以按 `Ctrl+Alt+P` 唤出控制台。

## 可以做什么

- 导入本地 PNG 皮肤，或输入 Java 版正版用户名 / UUID 获取皮肤；支持经典和纤细手臂、旧版及高清皮肤、皮肤外层。
- 站立、坐下，或分别自定义站姿和坐姿。身体和四肢可以旋转，不做肘膝弯曲。
- 头部跟随鼠标，水平与垂直方向的灵敏度、范围和中心点均可调整；整体角度也可调整。
- 站立时按住 Shift 立即潜行；坐下时随全局键盘输入敲击键盘，支持精准位置和双手交替。
- 长按按键保持手位，松开后自然回位。打字时短暂压低身体，幅度可调；快速敲字会重新播放动作。
- 点击鼠标挥动主手，可切换左右手；连续点击会立即重新挥动。
- 置顶、缩放、自由摆放和完全鼠标穿透。
- 可选随 Windows 登录自动启动，通过控制台或右键菜单开启、关闭。
- 用正版 ID 获取皮肤时，会一并取下账号当前佩戴的披风并显示在背后。穿透时鼠标移到桌宠上，会在约 0.15 秒内淡至 25% 不透明度。
- 可选 **hyp快速队伍聊天**：键盘输入半角 `#` 自动替换成 `/p chat `，不自动发送。
- 30 / 60 / 90 / 120 / 144 / 165 / 240 FPS 七档，没有持续动画时静止停止绘制。设置和导入的皮肤自动保存。

## v2.5 新增

Q 版小人、可选 DA 眨眼与扭腰、四种主手映射、喂食苹果、糖果色粗体浮字、音乐音符、可调戏、持续呼吸动画、系统深色模式及手动检查更新。

特别感谢 [zsyghxf](https://github.com/zsyghxf) 的 [PR #5](https://github.com/DLmember/Minecraft-Desktop-Pet/pull/5)，提供眨眼、Shift 动画与 Q 版模式的基础实现；DLmember 完成行为调整、冲突修复、界面整理及新增互动。逐项归属与完整变化见 [v2.5 发布说明](发布说明-v2.5.md)。

Linux 分支（`linux/` 前端、X11 / Wayland 适配、deb 与 Arch 打包、全屏鼠标跟随桥接）由 Bilibili [灰白happygray](https://space.bilibili.com/3690971449526321) 开发。

完整操作见 [使用说明](使用说明.md)，版本变化见 [更新记录](CHANGELOG.md)。

## 隐私与网络

日常互动在本机处理，正式运行不记录输入文本、不上传按键。主动获取正版皮肤时连接 Mojang 的账号资料及纹理服务；点击“检查更新”时连接 GitHub。无需账号密码。开启音乐音符时仅在本机查询音频会话，不录音、不上传音频。

## 从源码构建（macOS）

本地移植复用原始 Rust 渲染、皮肤与动画，通过 Swift / AppKit 提供透明窗口、菜单栏、控制台及全局键盘互动，以及可选的井号聊天前缀替换。
支持 macOS 12+，包含 Apple Silicon / Intel 通用架构；支持需辅助功能授权的 hyp 快速队伍聊天。
2.4.12 的 macOS 功能补齐包括正版披风下载与显示、登录自启、0–120 秒延迟和五档渲染线程 QoS（macOS 调度机制，与 Windows 进程优先级并非完全等价）。
完整操作说明参见 [macOS 说明](macos/使用说明-macOS.md)。

构建需要 Xcode Command Line Tools、Swift 和 Rust stable：

    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo test --manifest-path native/Cargo.toml --lib --locked
    scripts/build-macos.sh /absolute/path/to/output

默认产物为 release/macos/Minecraft桌宠.app 及 ZIP。构建脚本使用本地临时签名，未进行 Apple 公证。

## 从源码构建（Windows）

Windows 原生 Rust + Win32 实现，使用 CPU 绘制透明方块模型，不包含浏览器运行时。

1. 安装 Rust 的 `stable-x86_64-pc-windows-gnu` 工具链。
2. 安装 x86_64 UCRT 版 [LLVM-MinGW](https://github.com/mstorsjo/llvm-mingw/releases)，其中需要 `clang++.exe`、`windres.exe` 和 `x86_64-w64-mingw32-gcc.exe`。
3. 建议将项目放在不含中文或空格的路径，运行：

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build.ps1 -ToolchainBin C:\llvm-mingw\bin
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build.ps1 -Action test -ToolchainBin C:\llvm-mingw\bin
```

产物：`native/target/release/minecraft-pet.exe`。脚本使用 Cargo.lock 锁定的依赖；首次构建需要网络下载依赖。可通过 `-RustToolchain` 选择已安装的 GNU 工具链版本。

源码位于 `native/src`；首次启动预设位于 `native/default-settings.json`；内置皮肤及图标位于 `assets`。

## 从源码构建（Linux）

> **Linux 分支**由 Bilibili [灰白happygray](https://space.bilibili.com/3690971449526321) 开发维护，前端源码在 `linux/`。上游主分支只含 Windows / macOS 实现。

Linux 版复用同一套 Rust 渲染器与设置格式，新增 `linux/` 前端：X11 32 位 ARGB 透明置顶窗口、Shape 鼠标穿透、全屏鼠标跟随（Wayland 下经 KWin 脚本桥接光标）、XRecord / evdev 全局键鼠监听、StatusNotifier 托盘、GTK3 控制台、XDG 登录自启与正版皮肤获取。

依赖（Debian / Ubuntu）：

```bash
sudo apt install build-essential pkg-config libgtk-3-dev
```

构建与运行：

```bash
cargo build --release --manifest-path linux/Cargo.toml
./linux/target/release/minecraft-desktop-pet
# 或打包
scripts/build-linux.sh /absolute/path/to/output
```

要求 X11 或 Wayland 下的 XWayland、带 32 位 ARGB visual 的合成器，以及 Shape / RECORD 扩展。左键单击托盘图标打开控制台，右键打开快捷菜单；穿透时也可以按 `Ctrl+Alt+P`，或者再运行一次程序，唤出已在运行的实例的控制台。Wayland 会话下鼠标跟随由 KWin 脚本补齐（整个屏幕都能跟随），全局键盘需要额外配置（见 `linux/README.md`）。`hypQuickChat` 和音乐音符在 Linux 上不可用。

完整操作、命令行参数与限制见 [Linux 说明](linux/README.md)。

## 许可

应用代码及项目原创示例皮肤采用 [MIT License](LICENSE)。依赖许可证见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 和 `native/licenses/`。用户导入的皮肤不受本项目许可证覆盖，请尊重原作者授权。
