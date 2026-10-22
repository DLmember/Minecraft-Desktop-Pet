# Minecraft桌宠 · Blockbench兼容版 v2.4.12-blockbench.1

单独提供的 Windows 兼容版本，供需要导入 Blockbench 模型的用户使用。

## 特别感谢贡献者

**本版本的 Blockbench 模型与骨骼动画支持，由社区贡献者 [@RuiShu1024](https://github.com/RuiShu1024) 开发，并通过 [PR #4 — Feature/blockbench support](https://github.com/DLmember/Minecraft-Desktop-Pet/pull/4) 提交。**

特别感谢 RuiShu1024 为桌宠带来 Blockbench 支持，以及在模型渲染、骨骼动画和交互适配上付出的时间与努力！这项功能来自 ta 的贡献。

本次兼容版在该贡献基础上进行了与当前版本的冲突处理、独立版本打包，以及若干稳定性和交互问题修复。

## 新增支持

- 导入以方块为主体的 Blockbench `.bbmodel` 模型，以及基岩版 `.geo.json` 几何模型。
- 模型动画选择、播放速度和循环设置。
- 按约定命名的骨骼支持视线跟随、坐姿和键盘互动；实际效果取决于模型结构。
- 控制台新增 Blockbench 页面，可切换回普通 Minecraft 玩家皮肤。

## 本次整合修复

- 保留 GitHub 入口和开机自启，不加入启动延迟、进程优先级选项。
- 修复关闭压低效果同时禁用打字手臂响应、站姿持续按键重复重启动画的问题。
- 修复无输入时模型动画停止刷新、动画下拉框切换无效的问题。
- 导入后单独保存模型与贴图，避免移动原文件后重启丢失。
- 增加贴图大小、模型复杂度和骨骼循环引用检查。

## 使用

运行 `Minecraft桌宠-Blockbench兼容版.exe`，进入控制台第四页「Blockbench」导入模型。
设置独立保存在 `%APPDATA%\minecraft-desktop-pet-blockbench`，不会覆盖普通版设置。
支持范围、模型限制及制作要求见随包说明；不代表支持 Blockbench 的所有格式、插件、网格或表达式。

本版是 Windows 本地测试版本，本次未提供对应 macOS 构建。
本项目与 Mojang、Microsoft 及 Blockbench 官方均无隶属或授权关系。
