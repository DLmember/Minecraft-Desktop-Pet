# Minecraft桌宠 · Blockbench兼容版

版本：2.4.12-blockbench.1（Windows x64 本地测试版）

Blockbench 支持由 RuiShu1024 在 PR #4 中贡献，特别感谢 ta！详见「Release介绍.md」。

直接运行 Minecraft桌宠-Blockbench兼容版.exe，双击桌宠打开控制台，在第四页「Blockbench」导入模型、选择动画；也可恢复玩家皮肤。
支持以方块为主体的 .bbmodel 与基岩版 .geo.json 几何模型，不是任意 Blockbench 插件、网格或表达式的通用播放器。制作规范见「Blockbench模型制作指南.md」。

设置、皮肤、模型位于 %APPDATA%\minecraft-desktop-pet-blockbench，与普通版分开保存。首次运行使用内置默认设置，不复制私人皮肤。
导入后的模型及贴图会另行保存，移动原始文件不影响已导入模型。开机自启请将 EXE 放在固定位置。

模型文件最多 8 MiB、2048 方块、256 骨骼；贴图总像素最多 262144（相当于一张 512×512），单边最多 1024。
复杂模型持续播放会增加 CPU 和内存使用，不保证任意模型达到设定帧率或普通版内存目标。
附带 example_duck.bbmodel 用于导入测试；原示例贴图损坏，已替换成简单黄色测试贴图。

原项目由 GPT 6 Astra 制作。Blockbench 功能源于 RuiShu1024 的贡献。
本项目与 Mojang、Microsoft 及 Blockbench 官方均无隶属或授权关系。
