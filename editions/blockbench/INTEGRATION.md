# 独立兼容版整合记录

- 基础 main：bcedc4b3f5c3b627f3e43758fd76acd60ca043ba
- PR #4：306aac29f2f9212c1fa244792585cfecd70c1cfb
- 作者：RuiShu1024；https://github.com/DLmember/Minecraft-Desktop-Pet/pull/4
- online.rs 格式冲突保留 main；ui.rs 保留模型页与 GitHub 入口，通用页脚归属为 255。
- 恢复动画组合框 504 及其通知分发，不恢复已删除的 161/162 选项。
- 使用独立设置目录、自启项及窗口类。
- Windows 单元测试 66 项通过。PR 的 render_test_frames 在贡献者本机文件不存在时直接返回，不代表实机覆盖。
- 未执行远端推送或合并。

实机启动检查通过：独立标题、第四页模型设置、动画下拉框切换、GitHub入口、无旧延迟/优先级选项；无输入时 1.2 秒继续绘制 147 帧。
