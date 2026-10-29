import AppKit
import UniformTypeIdentifiers

enum ConsoleSize {
    static let design = NSSize(width: 760, height: 780)
    static let compact = NSSize(width: 646, height: 663)
}

extension AppDelegate {
    @objc func showSettings() {
        guard core != nil else { return }
        if startupPending { finishStartup(); return }
        if let panel { panel.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true); return }
        let w = consoleWindow ?? NSWindow(contentRect: NSRect(origin: .zero, size: ConsoleSize.compact),
            styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        consoleWindow = w
        w.setContentSize(ConsoleSize.compact)
        w.contentView = NSView(frame: NSRect(origin: .zero, size: ConsoleSize.compact))
        w.title = "Minecraft桌宠 · v\(appVersion) · 控制台"
        w.isReleasedWhenClosed = false; w.isRestorable = false; w.delegate = self
        let root = w.contentView!
        // Scale the existing layout as one view, including native hit testing.
        root.setBoundsSize(ConsoleSize.design)
        for (index, title) in ["常规", "视线与角度", "自定义姿态"].enumerated() {
            let button = NSButton(title: title, target: self, action: #selector(selectConsolePage(_:)))
            button.setButtonType(.pushOnPushOff); button.bezelStyle = .regularSquare
            button.font = .systemFont(ofSize: 15, weight: .medium); button.tag = index
            button.frame = NSRect(x: 24 + index * 246, y: 728, width: 220, height: 34)
            button.setAccessibilityLabel(title); root.addSubview(button); pageButtons.append(button)
        }
        let tabs = NSTabView(frame: NSRect(x: 24, y: 114, width: 712, height: 596))
        tabs.tabViewType = .noTabsNoBorder
        for title in ["常规", "视线与角度", "自定义姿态"] {
            let item = NSTabViewItem(identifier: title); item.label = title; tabs.addTabViewItem(item)
        }
        root.addSubview(tabs); controlTabs = tabs; panel = w; tabs.delegate = self
        uiReady = false; tabs.selectTabViewItem(at: min(selectedConsolePage, 2)); uiReady = true
        populatePage(min(selectedConsolePage, 2))
        action(root, "检查更新", #selector(checkUpdates), 20, 70, 112)
        action(root, "输入权限…", #selector(showPermissions), 145, 70, 112)
        action(root, "恢复默认…", #selector(confirmReset), 270, 70, 112)
        let github = action(root, "GitHub", #selector(openProject), 614, 70, 126)
        if let url = Bundle.main.url(forResource: "github", withExtension: "svg"), let icon = NSImage(contentsOf: url) {
            icon.size = NSSize(width: 18, height: 18); icon.isTemplate = true
            github.image = icon; github.imagePosition = .imageLeading
        }
        github.setAccessibilityLabel("GitHub 项目主页")
        saveStatus = label(root, "已保存", 24, 43, 80)
        permissionSummary = label(root, "", 112, 43, 420)
        permissionSummary?.font = .systemFont(ofSize: 11); permissionSummary?.textColor = .secondaryLabelColor
        let credit = label(root, "由 GPT 6 Astra 制作", 534, 43, 202)
        credit.alignment = .right
        label(root, "拖动摆放 · 右键菜单 · 双击设置；关闭此窗口不会退出桌宠。", 24, 16, 712).textColor = .secondaryLabelColor
        updatePermission(); w.center(); w.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
    }
    @objc func selectConsolePage(_ sender: NSButton) {
        // Selecting the current tab does not call NSTabViewDelegate again.
        // Keep its push-on/off button selected instead of leaving all tabs off.
        guard sender.tag != selectedConsolePage else { sender.state = .on; return }
        controlTabs?.selectTabViewItem(at: sender.tag)
    }
    func releaseConsole() {
        uiReady = false; bindings.removeAll(); pageButtons.removeAll(); saveStatus = nil
        controlTabs = nil; panel = nil
        permissionSummary = nil; skinSummary = nil; onlineField = nil; onlineStatus = nil
        permissionLabel = nil; startupStatus = nil; musicToggle = nil
        consoleWindow?.contentView = NSView(frame: NSRect(origin: .zero, size: ConsoleSize.compact))
    }
    func windowWillClose(_ notification: Notification) {
        guard let window = notification.object as? NSWindow else { return }
        if window === panel {
            closePermissions(); save(); selectedConsolePage = controlTabs?.indexOfTabViewItem(controlTabs!.selectedTabViewItem!) ?? 0
            releaseConsole()
        }
    }
    func tabView(_ tabView: NSTabView, didSelect tabViewItem: NSTabViewItem?) {
        guard uiReady, let item = tabViewItem else { return }
        populatePage(tabView.indexOfTabViewItem(item))
    }
    func populatePage(_ index: Int) {
        guard let tabs = controlTabs else { return }
        bindings.removeAll(); skinSummary = nil; onlineField = nil; onlineStatus = nil; musicToggle = nil
        for (i, item) in tabs.tabViewItems.enumerated() where i != index { item.view = NSView() }
        selectedConsolePage = index
        for button in pageButtons { button.state = button.tag == index ? .on : .off }
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 712, height: 596))
        tabs.tabViewItems[index].view = content
        switch index {
        case 1: addHead(content)
        case 2: addPose(content)
        default: addGeneral(content)
        }
    }
    @discardableResult
    func label(_ view: NSView, _ text: String, _ x: CGFloat, _ y: CGFloat, _ width: CGFloat, _ height: CGFloat = 20) -> NSTextField {
        let l = NSTextField(labelWithString: text)
        l.font = .systemFont(ofSize: 13)
        l.frame = NSRect(x: x, y: y, width: width, height: height)
        view.addSubview(l); return l
    }
    @discardableResult
    func action(_ view: NSView, _ title: String, _ selector: Selector, _ x: CGFloat, _ y: CGFloat, _ width: CGFloat) -> NSButton {
        let b = NSButton(title: title, target: self, action: selector); b.bezelStyle = .rounded
        b.frame = NSRect(x: x, y: y, width: width, height: 28); view.addSubview(b); return b
    }
    func check(_ view: NSView, _ title: String, _ key: String, _ x: CGFloat, _ y: CGFloat, _ width: CGFloat = 290) {
        let b = NSButton(checkboxWithTitle: title, target: self, action: #selector(controlChanged(_:)))
        b.font = .systemFont(ofSize: 13); b.state = flag(key) ? .on : .off
        b.frame = NSRect(x: x, y: y, width: width, height: 24)
        if key == "daBlink" { b.isEnabled = !flag("chibi"); b.toolTip = "需要专门修改过的皮肤；Q 版自动关闭眨眼。" }
        if key == "musicNotes" { musicToggle = b; refreshMusicLabel() }
        bindings[ObjectIdentifier(b)] = Binding(key); view.addSubview(b)
    }
    func popup(_ view: NSView, _ title: String, _ key: String, _ names: [String], _ values: [Any],
               _ x: CGFloat, _ y: CGFloat, _ width: CGFloat = 290) {
        label(view, title, x, y + 4, 88)
        let b = NSPopUpButton(frame: NSRect(x: x + 90, y: y, width: width - 90, height: 28), pullsDown: false)
        b.addItems(withTitles: names)
        let current = String(describing: settings[key] ?? "")
        if let i = values.firstIndex(where: { String(describing: $0) == current }) { b.selectItem(at: i) }
        b.target = self; b.action = #selector(controlChanged(_:))
        bindings[ObjectIdentifier(b)] = Binding(key, values); view.addSubview(b)
    }
    func radios(_ view: NSView, _ key: String, _ names: [String], _ choices: [String],
                _ x: CGFloat, _ y: CGFloat, _ step: CGFloat) {
        // Each row has its own container so AppKit's native radio grouping does
        // not deselect unrelated rows. The selected value stays in the binding.
        let group = NSView(frame: NSRect(x: x, y: y, width: step * CGFloat(names.count), height: 24))
        view.addSubview(group)
        for (index, title) in names.enumerated() {
            let button = NSButton(radioButtonWithTitle: title, target: self, action: #selector(controlChanged(_:)))
            button.frame = NSRect(x: CGFloat(index) * step, y: 0, width: step - 2, height: 24)
            button.font = .systemFont(ofSize: 13)
            button.state = settings[key] as? String == choices[index] ? .on : .off
            button.setAccessibilityLabel(title)
            bindings[ObjectIdentifier(button)] = Binding(key, [choices[index]])
            group.addSubview(button)
        }
    }
    func displayValue(_ key: String, _ value: Double) -> String {
        switch key {
        case "scale": return String(format: "%.0f%%", value * 100)
        case "typingSquash", "chibiBodyScale": return String(format: "%.0f%%", value)
        case "fps": return "\(Int(value)) FPS"
        default: return String(format: "%.1f", value)
        }
    }
    func syncRadioStates(_ view: NSView) {
        if let button = view as? NSButton,
           let binding = bindings[ObjectIdentifier(button)], let choice = binding.choices.first as? String {
            button.state = settings[binding.key] as? String == choice ? .on : .off
        }
        for child in view.subviews { syncRadioStates(child) }
    }
    func generalSlider(_ view: NSView, _ title: String, _ key: String, _ lower: Double,
                       _ upper: Double, _ y: CGFloat, choices: [Int] = []) {
        label(view, title, 0, y + 3, 120)
        let current = value(key)
        let output = label(view, displayValue(key, current), 626, y + 3, 86)
        output.alignment = .right; output.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        let index = choices.firstIndex(of: Int(current)) ?? 0
        let slider = NSSlider(value: choices.isEmpty ? current : Double(index), minValue: lower,
            maxValue: upper, target: self, action: #selector(controlChanged(_:)))
        slider.frame = NSRect(x: 126, y: y, width: 484, height: 24); slider.isContinuous = true
        slider.setAccessibilityLabel(title)
        if !choices.isEmpty { slider.numberOfTickMarks = choices.count; slider.allowsTickMarkValuesOnly = true }
        bindings[ObjectIdentifier(slider)] = Binding(key, choices.map { $0 as Any }, output)
        view.addSubview(slider)
    }
    func value(_ key: String) -> Double {
        let p = key.split(separator: ".").map(String.init)
        if p.count == 1 { return number(key) }
        if p.count == 2, let i = Int(p[1]), let list = settings[p[0]] as? [Double], list.indices.contains(i) { return list[i] }
        if p.count == 3, let i = Int(p[2]), let dict = settings[p[0]] as? [String: Any], let list = dict[p[1]] as? [Double], list.indices.contains(i) { return list[i] }
        return 0
    }
    func setValue(_ key: String, _ value: Double) {
        let p = key.split(separator: ".").map(String.init)
        if p.count == 1 { settings[key] = value }
        else if p.count == 2, let i = Int(p[1]), var list = settings[p[0]] as? [Double], list.indices.contains(i) { list[i] = value; settings[p[0]] = list }
        else if p.count == 3, let i = Int(p[2]), var dict = settings[p[0]] as? [String: Any], var list = dict[p[1]] as? [Double], list.indices.contains(i) {
            list[i] = value; dict[p[1]] = list; settings[p[0]] = dict
        }
    }
    func slider(_ view: NSView, _ title: String, _ key: String, _ min: Double, _ max: Double,
                _ x: CGFloat, _ y: CGFloat, _ width: CGFloat = 590) {
        label(view, title, x, y + 3, 110)
        let v = value(key), out = label(view, String(format: "%.1f", value(key)), x + width - 45, y + 3, 45)
        out.alignment = .right; out.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
        let s = NSSlider(value: v, minValue: min, maxValue: max, target: self, action: #selector(controlChanged(_:)))
        s.frame = NSRect(x: x + 114, y: y, width: width - 168, height: 24)
        s.isContinuous = true; bindings[ObjectIdentifier(s)] = Binding(key, [], out); view.addSubview(s)
    }
    @objc func controlChanged(_ sender: NSControl) {
        guard let b = bindings[ObjectIdentifier(sender)] else { return }
        if let s = sender as? NSSlider {
            let v: Double
            if !b.choices.isEmpty {
                let index = min(b.choices.count - 1, max(0, Int(s.doubleValue.rounded())))
                v = (b.choices[index] as? NSNumber)?.doubleValue ?? s.doubleValue
            } else { v = s.doubleValue }
            setValue(b.key, v); b.label?.stringValue = displayValue(b.key, v)
        }
        else if let p = sender as? NSPopUpButton, b.choices.indices.contains(p.indexOfSelectedItem) { settings[b.key] = b.choices[p.indexOfSelectedItem] }
        else if let c = sender as? NSButton {
            if let choice = b.choices.first {
                settings[b.key] = choice
                for peer in c.superview?.subviews.compactMap({ $0 as? NSButton }) ?? [] {
                    peer.state = peer === c ? .on : .off
                }
            } else { settings[b.key] = c.state == .on }
        }
        if b.key == "autostart" {
            do { if !smoke { try syncLoginStartup() } }
            catch { loadSettings(); (sender as? NSButton)?.state = flag("autostart") ? .on : .off; alert(error.localizedDescription); return }
        }
        if b.key.hasPrefix("custom") { settings["posture"] = "custom" }
        if let page = controlTabs?.selectedTabViewItem?.view { syncRadioStates(page) }
        if b.key == "chibi" && flag("chibi") { settings["daBlink"] = false }
        if b.key == "typingLetters" && flag("typingLetters") { prepareGlyphs() }
        changed()
        if b.key == "autostart" { save() }
        if b.key == "musicNotes" { configureMusic() }
        if b.key == "keyboardEnabled" || b.key == "hypQuickChat" {
            save(); configureInput()
            if !smoke && b.key == "keyboardEnabled" && flag("keyboardEnabled") && !input.running { grantInput() }
            if !smoke && b.key == "hypQuickChat" && flag("hypQuickChat") && !input.accessibilityAllowed { grantAccessibility() }
        }
        if b.key == "customBase" || b.key == "chibi" { populatePage(selectedConsolePage) }
    }
    func addGeneral(_ view: NSView) {
        label(view, "皮肤", 0, 551, 110)
        skinSummary = label(view, "", 126, 551, 320); updateSkinSummary()
        skinSummary?.lineBreakMode = .byTruncatingMiddle
        action(view, "导入皮肤…", #selector(importSkin), 466, 546, 124)
        action(view, "恢复内置", #selector(restoreBuiltinSkin), 598, 546, 118)
        label(view, "正版 ID", 0, 515, 110)
        let field = NSTextField(frame: NSRect(x: 126, y: 512, width: 334, height: 25))
        field.placeholderString = "Java 版用户名或 UUID"; view.addSubview(field); onlineField = field
        action(view, "获取皮肤", #selector(downloadSkin), 466, 509, 124)
        label(view, "无需登录", 602, 515, 110)
        onlineStatus = label(view, "", 126, 488, 586)
        onlineStatus?.font = .systemFont(ofSize: 11); onlineStatus?.textColor = .secondaryLabelColor
        for (title, y) in [("手臂",452),("外观",420),("姿势",388),("敲字",356),("主手",324),
                            ("桌面",292),("键盘",260),("动画",228),("其他",36),("音乐",4)] {
            label(view, title, 0, CGFloat(y + 3), 110)
        }
        radios(view, "armModel", ["经典（4 px）", "纤细（3 px）"], ["classic","slim"], 126, 452, 230)
        check(view, "显示皮肤外层", "outerLayer", 126, 420, 260)
        check(view, "Q 版小人", "chibi", 478, 420, 210)
        radios(view, "posture", ["站立","坐下","自定义"], ["standing","sitting","custom"], 126, 388, 112)
        label(view, "站立时按住 Shift 潜行", 478, 391, 234)
        radios(view, "typingMode", ["精准按键","双手交替"], ["precise","alternating"], 126, 356, 230)
        label(view, "坐姿下响应键盘", 478, 359, 234)
        radios(view, "mainHand", ["右手挥动","左手挥动","实际映射","反向映射"],
               ["right","left","mapped","reverse"], 126, 324, 146)
        check(view, "保持置顶", "alwaysOnTop", 126, 292, 210)
        check(view, "完全鼠标穿透", "clickThrough", 356, 292, 220)
        action(view, "找回桌宠", #selector(recenter), 598, 289, 118)
        check(view, "全局键盘互动", "keyboardEnabled", 126, 260, 220)
        check(view, "hyp 快速队伍聊天", "hypQuickChat", 356, 260, 220)
        label(view, "# → /p chat", 594, 263, 118)
        check(view, "DA 眨眼（需修改过皮肤）", "daBlink", 126, 228, 274)
        check(view, "DA 扭腰", "daDance", 418, 228, 126)
        check(view, "敲字浮现字", "typingLetters", 560, 228, 152)
        check(view, "可调戏", "teaseEnabled", 126, 196, 210)
        check(view, "呼吸动画", "idleAnimation", 356, 196, 138)
        label(view, "开启后持续轻微呼吸", 500, 199, 212)
        generalSlider(view, "大小", "scale", 0.65, 1.8, 164)
        generalSlider(view, "帧率", "fps", 0, 6, 132, choices: [30,60,90,120,144,165,240])
        generalSlider(view, "打字压低", "typingSquash", 0, 30, 100)
        generalSlider(view, "Q 版身体大小", "chibiBodyScale", 50, 180, 68)
        check(view, "显示正版披风", "showCape", 126, 36, 260)
        check(view, "随 macOS 登录自动启动", "autostart", 412, 36, 300)
        check(view, "播放音乐时冒音符", "musicNotes", 126, 4, 280)
        popup(view, "登录延迟", "autostartDelay", ["立即","5 秒","15 秒","30 秒","60 秒","120 秒"],
              LoginStartup.delays, 412, 1, 300)
    }
    func addHead(_ view: NSView) {
        label(view, "整体角度偏移", 0, 552, 300)
        action(view, "角度归零", #selector(resetRotation), 598, 546, 118)
        for (i, title) in ["前后俯仰 · X","左右转向 · Y","左右倾斜 · Z"].enumerated() {
            generalSlider(view, title, "modelRotation.\(i)", -180, 180, CGFloat(500 - i * 43))
        }
        check(view, "视线跟随鼠标", "trackMouse", 0, 350, 300)
        action(view, "跟随复位", #selector(resetHead), 598, 346, 118)
        for (x, axis, title) in [(CGFloat(0), "X", "水平 · 左右"), (CGFloat(380), "Y", "垂直 · 上下")] {
            label(view, title, x, 310, 332)
            for (row, entry) in [("灵敏度", "headSensitivity", 0.0, 300.0),
                                 ("转动范围", "headRange", 0.0, 85.0),
                                 ("中心偏移", "headCenter", -60.0, 60.0)].enumerated() {
                let y = CGFloat(264 - row * 65), key = entry.1 + axis
                label(view, entry.0, x, y, 200)
                let out = label(view, displayValue(key, value(key)), x + 244, y, 88); out.alignment = .right
                let slider = NSSlider(value: value(key), minValue: entry.2, maxValue: entry.3,
                    target: self, action: #selector(controlChanged(_:)))
                slider.frame = NSRect(x: x, y: y - 28, width: 332, height: 24); slider.isContinuous = true
                slider.setAccessibilityLabel(title + " · " + entry.0)
                bindings[ObjectIdentifier(slider)] = Binding(key, [], out); view.addSubview(slider)
            }
        }
        label(view, "负值偏左，正值偏右", 0, 60, 332).textColor = .secondaryLabelColor
        label(view, "负值抬头，正值低头", 380, 60, 332).textColor = .secondaryLabelColor
    }
    func addPose(_ view: NSView) {
        radios(view, "posture", ["使用自定义姿态"], ["custom"], 0, 548, 245)
        radios(view, "customBase", ["站姿","坐姿"], ["standing","sitting"], 266, 548, 116)
        action(view, "姿态归零", #selector(resetPose), 598, 546, 118)
        label(view, "站姿和坐姿分别保存；坐姿角度在默认坐姿上调整。", 0, 508, 712)
        for (i, axis) in ["前后旋转 · X", "左右旋转 · Y", "侧向旋转 · Z"].enumerated() {
            label(view, axis, CGFloat(98 + i * 205), 460, 200).alignment = .center
        }
        let key = settings["customBase"] as? String == "sitting" ? "customSittingPose" : "customPose"
        for (i, part) in [("head","头部"),("body","身体"),("leftArm","左臂"),("rightArm","右臂"),("leftLeg","左腿"),("rightLeg","右腿")].enumerated() {
            let y = CGFloat(409 - i * 54); label(view, part.1, 0, y + 3, 85)
            for axis in 0..<3 {
                let x = CGFloat(98 + axis * 205), path = "\(key).\(part.0).\(axis)"
                let out = label(view, displayValue(path, value(path)), x + 142, y + 3, 58)
                out.alignment = .right; out.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
                let slider = NSSlider(value: value(path), minValue: -180, maxValue: 180,
                    target: self, action: #selector(controlChanged(_:)))
                slider.frame = NSRect(x: x, y: y, width: 136, height: 24); slider.isContinuous = true
                slider.setAccessibilityLabel(part.1 + " " + ["X","Y","Z"][axis])
                bindings[ObjectIdentifier(slider)] = Binding(path, [], out); view.addSubview(slider)
            }
        }
        label(view, "坐姿支持敲字、长按和挥手；互动结束回到所设姿态。", 0, 64, 712).textColor = .secondaryLabelColor
        label(view, "头部角度在关闭视线跟随后生效；不弯曲肢体。", 0, 38, 712).textColor = .secondaryLabelColor
    }
    func syncLoginStartup() throws {
        try LoginStartup.setEnabled(flag("autostart"), app: Bundle.main.bundleURL, data: dataDir, directory: loginDirectory)
    }
    func updateSkinSummary() { skinSummary?.stringValue = settings["skinName"] as? String ?? "内置" }
    @objc func importSkin() {
        let p = NSOpenPanel(); p.allowedContentTypes = [.png]; p.allowsMultipleSelection = false; p.canChooseDirectories = false
        NSApp.activate(ignoringOtherApps: true)
        if p.runModal() == .OK, let url = p.url {
            downloadID = UUID(); save()
            guard url.path.withCString({ pet_import(core, $0) }) else { showCoreError(); return }
            loadSettings(); wake(); populatePage(selectedConsolePage)
        }
    }
    @objc func restoreBuiltinSkin() {
        downloadID = UUID(); save()
        guard pet_restore_skin(core) else { showCoreError(); return }
        loadSettings(); updateSkinSummary(); wake()
    }
    @objc func resetHead() {
        for (key, value) in ["headSensitivityX":100.0, "headSensitivityY":100, "headRangeX":19.5,
                             "headRangeY":53, "headCenterX":0, "headCenterY":0] { settings[key] = value }
        changed(); populatePage(1)
    }
    @objc func resetRotation() { settings["modelRotation"] = [0.0,0,0]; changed(); populatePage(1) }
    @objc func resetPose() {
        let key = settings["customBase"] as? String == "sitting" ? "customSittingPose" : "customPose"
        settings[key] = Dictionary(uniqueKeysWithValues: ["head","body","leftArm","rightArm","leftLeg","rightLeg"].map { ($0, [0.0,0,0]) })
        changed(); populatePage(2)
    }
    @objc func confirmReset() {
        let a = NSAlert(); a.messageText = "恢复默认设置与内置皮肤？"
        a.informativeText = "这会重置桌宠偏好，关闭本应用的登录自启；原始皮肤文件保留。"
        a.addButton(withTitle: "取消"); a.addButton(withTitle: "恢复默认")
        if a.runModal() == .alertSecondButtonReturn { resetSettings() }
    }
    @objc func showPermissions() {
        guard permissionPanel == nil else { permissionPanel?.makeKeyAndOrderFront(nil); return }
        let w = NSPanel(contentRect: NSRect(x:0,y:0,width:580,height:270), styleMask:[.titled], backing:.buffered, defer:false)
        w.title = "输入权限与诊断"; let root = w.contentView!; permissionPanel = w
        accessLabel = label(root, "", 18, 214, 544, 44)
        accessLabel?.lineBreakMode = .byWordWrapping
        inputRequestButton = action(root, "申请输入监控", #selector(grantInput), 12, 176, 174)
        accessRequestButton = action(root, "申请辅助功能", #selector(grantAccessibility), 194, 176, 174)
        action(root, "重新检测", #selector(refreshPermissions), 380, 176, 178)
        inputHealthLabel = label(root, "", 18, 122, 544, 44); inputHealthLabel?.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        let field = NSTextField(frame:NSRect(x:18,y:83,width:542,height:26)); field.placeholderString = "开启快速聊天后，在这里输入 # 测试"; root.addSubview(field); diagnosticsField = field
        action(root, "重启应用", #selector(restartApplication), 12, 24, 142)
        action(root, "设置文件夹", #selector(openDataFolder), 164, 24, 142)
        action(root, "完成", #selector(closePermissions), 438, 24, 120)
        updatePermission()
        if let panel { panel.beginSheet(w) } else { w.center(); w.makeKeyAndOrderFront(nil) }
    }
    @objc func closePermissions() {
        if let w = permissionPanel { w.sheetParent?.endSheet(w); w.orderOut(nil) }
        permissionPanel = nil; accessLabel = nil; inputRequestButton = nil; accessRequestButton = nil
        inputHealthLabel = nil; diagnosticsField = nil
    }
    @objc func previewSwing() { handleMousePress(); wake() }
}
