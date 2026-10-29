import AppKit

/// One settings value carried by a menu item, so radio-style submenus share one action.
final class MenuSetting: NSObject {
    let key: String, value: Any
    init(_ key: String, _ value: Any) { self.key = key; self.value = value }
}

/// Pet right-click menu and status-bar menu share the Windows 2.5.2 layout:
/// same-kind options live in submenus whose title shows the current value.
/// The frame timer runs in `.common` mode, so the pet keeps animating while a menu tracks.
extension AppDelegate {
    func buildMenu(_ menu: NSMenu) {
        menu.removeAllItems()
        add(menu, "打开控制台…", #selector(showSettings), key: ",")
        if startupPending {
            add(menu, "退出 Minecraft桌宠", #selector(quit), key: "q")
            return
        }
        let actions = submenu(menu, "触发动作")
        for (index, title) in ["打招呼", "开心蹦蹦", "害羞", "鼓掌"].enumerated() {
            add(actions, title, #selector(triggerGesture(_:))).tag = index
        }
        add(actions, "喂食苹果", #selector(feedApple))
        menu.addItem(.separator())
        let posture = settings["posture"] as? String ?? "standing"
        let postureNames = ["standing": "站立", "sitting": "坐下", "custom": "自定义姿态"]
        choices(submenu(menu, "姿态：\(postureNames[posture] ?? "站立")"), "posture",
                [("站立", "standing"), ("坐下", "sitting"), ("自定义姿态", "custom")])
        let look = submenu(menu, "外观")
        add(look, "导入 PNG 皮肤…", #selector(importSkin))
        look.addItem(.separator())
        choices(look, "armModel", [("经典手臂", "classic"), ("纤细手臂", "slim")])
        look.addItem(.separator())
        toggle(look, "显示正版披风", "showCape")
        toggle(look, "Q 版小人", "chibi")
        let scale = number("scale")
        let sizes = submenu(menu, "大小：\(Int((scale * 100).rounded()))%")
        for value in [0.75, 1.0, 1.25, 1.5] {
            let item = add(sizes, "\(Int(value * 100))%", #selector(menuChoice(_:)), state: abs(scale - value) < 0.001)
            item.representedObject = MenuSetting("scale", value)
        }
        let hand = settings["mainHand"] as? String ?? "right"
        let handNames = ["right": "右手", "left": "左手", "mapped": "实际映射", "reverse": "反向映射"]
        choices(submenu(menu, "鼠标挥动：\(handNames[hand] ?? "右手")"), "mainHand",
                [("右手", "right"), ("左手", "left"), ("实际映射", "mapped"), ("反向映射", "reverse")])
        let typing = !flag("keyboardEnabled") ? "已关闭"
            : settings["typingMode"] as? String == "alternating" ? "双手交替" : "精准按键"
        let keys = submenu(menu, "键盘互动：\(typing)")
        toggle(keys, "全局键盘互动", "keyboardEnabled")
        keys.addItem(.separator())
        choices(keys, "typingMode", [("精准按键", "precise"), ("双手交替", "alternating")])
        keys.addItem(.separator())
        add(keys, "hyp 快速聊天：# → /p chat", #selector(toggleQuickChat), state: flag("hypQuickChat"))
        add(keys, "申请输入监控…", #selector(grantInput))
        add(keys, "申请辅助功能（快速聊天）…", #selector(grantAccessibility))
        menu.addItem(.separator())
        toggle(menu, "保持置顶", "alwaysOnTop")
        toggle(menu, "完全鼠标穿透", "clickThrough")
        toggle(menu, "视线跟随鼠标", "trackMouse")
        let desk = submenu(menu, "桌面与启动")
        add(desk, "移到屏幕中央", #selector(recenter))
        add(desk, hidden ? "显示桌宠" : "暂时隐藏桌宠", #selector(toggleHidden))
        desk.addItem(.separator())
        toggle(desk, "随 macOS 登录启动", "autostart")
        menu.addItem(.separator())
        add(menu, "退出 Minecraft桌宠", #selector(quit), key: "q")
    }
    func menuWillOpen(_ menu: NSMenu) { buildMenu(menu) }
    func showPetMenu(_ event: NSEvent, in view: NSView) {
        let menu = NSMenu(); buildMenu(menu)
        NSMenu.popUpContextMenu(menu, with: event, for: view)
    }
    @discardableResult
    private func add(_ menu: NSMenu, _ title: String, _ action: Selector, state: Bool? = nil, key: String = "") -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key); item.target = self
        if let state { item.state = state ? .on : .off }
        menu.addItem(item); return item
    }
    private func submenu(_ menu: NSMenu, _ title: String) -> NSMenu {
        let child = NSMenu(title: title), item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.submenu = child; menu.addItem(item); return child
    }
    private func toggle(_ menu: NSMenu, _ title: String, _ key: String) {
        add(menu, title, #selector(menuToggle(_:)), state: flag(key)).representedObject = key
    }
    private func choices(_ menu: NSMenu, _ key: String, _ options: [(String, String)]) {
        let current = settings[key] as? String
        for (title, value) in options {
            add(menu, title, #selector(menuChoice(_:)), state: current == value).representedObject = MenuSetting(key, value)
        }
    }
    @objc func menuChoice(_ sender: NSMenuItem) {
        guard let choice = sender.representedObject as? MenuSetting else { return }
        applyMenuSetting(choice.key, choice.value)
    }
    /// Flips the value at click time, so a menu item never writes a stale state.
    @objc func menuToggle(_ sender: NSMenuItem) {
        guard let key = sender.representedObject as? String else { return }
        applyMenuSetting(key, !flag(key))
    }
    /// Same side effects as the matching console control.
    func applyMenuSetting(_ key: String, _ value: Any) {
        let previous = settings[key]
        settings[key] = value
        if key == "autostart" {
            do { if !smoke { try syncLoginStartup() } }
            catch { settings[key] = previous; alert(error.localizedDescription); return }
        }
        changed()
        if key == "autostart" || key == "keyboardEnabled" { save() }
        if key == "keyboardEnabled" {
            configureInput()
            if !smoke && flag("keyboardEnabled") && !input.running { grantInput() }
        }
        refreshConsoleControls()
    }
    @objc func triggerGesture(_ sender: NSMenuItem) {
        guard core != nil, pet_gesture(core, UInt32(clamping: sender.tag), now) else { return }
        wake()
    }
}
