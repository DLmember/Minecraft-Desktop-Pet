import AppKit
import ApplicationServices
import UniformTypeIdentifiers

// Physical ANSI positions map to the original renderer's virtual-key layout.
let keyMap: [UInt16: UInt32] = [
    0:65,1:83,2:68,3:70,4:72,5:71,6:90,7:88,8:67,9:86,11:66,
    12:81,13:87,14:69,15:82,16:89,17:84,18:49,19:50,20:51,21:52,
    22:54,23:53,24:187,25:57,26:55,27:189,28:56,29:48,30:221,
    31:79,32:85,33:219,34:73,35:80,36:13,37:76,38:74,39:222,
    40:75,41:186,42:220,43:188,44:191,45:78,46:77,47:190,48:9,
    49:32,50:192,51:8,53:27,54:92,55:91,56:160,57:20,58:164,
    59:162,60:161,61:165,62:163,63:93,65:110,67:106,69:107,
    71:144,75:111,76:0x1000,78:109,81:187,82:96,83:97,84:98,
    85:99,86:100,87:101,88:102,89:103,91:104,92:105,96:116,
    97:117,98:118,99:114,100:119,101:120,103:122,105:124,106:127,
    107:125,109:121,111:123,113:126,114:45,115:36,116:33,117:46,
    118:115,119:35,120:113,121:34,122:112,123:37,124:39,125:40,126:38
]

final class PetPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}
final class PetView: NSView {
    weak var owner: AppDelegate?
    var bitmap: CGImage?
    private var dragOffset: NSPoint?
    private var didDrag = false
    override var isFlipped: Bool { true }
    override func draw(_ dirtyRect: NSRect) {
        guard let bitmap, let ctx = NSGraphicsContext.current?.cgContext else { return }
        ctx.clear(bounds)
        ctx.saveGState()
        ctx.interpolationQuality = .none
        ctx.translateBy(x: 0, y: bounds.height)
        ctx.scaleBy(x: 1, y: -1)
        ctx.draw(bitmap, in: bounds)
        ctx.restoreGState()
    }
    override func mouseDown(with event: NSEvent) {
        guard let owner, let window else { return }
        if event.clickCount == 2 { owner.showSettings(); return }
        // Local event monitor already dispatched exactly one swing.
        let cursor = NSEvent.mouseLocation
        dragOffset = NSPoint(x: cursor.x-window.frame.minX, y: cursor.y-window.frame.minY)
        didDrag = false
    }
    override func mouseDragged(with event: NSEvent) {
        guard let offset = dragOffset, let window else { return }
        let cursor = NSEvent.mouseLocation
        window.setFrameOrigin(NSPoint(x: cursor.x-offset.x, y: cursor.y-offset.y))
        didDrag = true; owner?.wake()
    }
    override func mouseUp(with event: NSEvent) {
        if didDrag { owner?.savePosition() }
        dragOffset = nil; didDrag = false
    }
    override func rightMouseDown(with event: NSEvent) { owner?.showSettings() }
}

final class Binding {
    let key: String
    let choices: [Any]
    weak var label: NSTextField?
    init(_ key: String, _ choices: [Any] = [], _ label: NSTextField? = nil) {
        self.key = key; self.choices = choices; self.label = label
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate, NSMenuDelegate, NSTabViewDelegate {
    var core: OpaquePointer!
    var settings: [String: Any] = [:]
    var pet: PetPanel!
    var view: PetView!
    var status: NSStatusItem!
    var panel: NSWindow?
    var timer: Timer?
    var saveTimer: Timer?
    let input = InputController()
    var permissionTimer: Timer?
    var permissionSummary: NSTextField?
    var accessLabel: NSTextField?
    var inputRequestButton: NSButton?
    var accessRequestButton: NSButton?
    var diagnosticsField: NSTextField?
    var inputHealthLabel: NSTextField?
    var controlTabs: NSTabView?
    var mousePresses: UInt64 = 0
    var mouseMonitor: Any?
    var localMouseMonitor: Any?
    var sleepObservers: [NSObjectProtocol] = []
    var bindings: [ObjectIdentifier: Binding] = [:]
    var permissionLabel: NSTextField?
    var onlineField: NSTextField?
    var onlineStatus: NSTextField?
    var lastCursor = NSPoint(x: -9999, y: -9999)
    var lastShift = false
    var lastDraw: Float = 0
    var awakeUntil: Float = 1
    var hidden = false
    var lastTick = ProcessInfo.processInfo.systemUptime
    let start = ProcessInfo.processInfo.systemUptime
    let smoke = CommandLine.arguments.contains("--smoke-test") || CommandLine.arguments.contains("--self-test-ui")
    let dataDir: URL = {
        if let i = CommandLine.arguments.firstIndex(of: "--data-dir"), CommandLine.arguments.count > i + 1 {
            return URL(fileURLWithPath: CommandLine.arguments[i + 1], isDirectory: true)
        }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("MinecraftDesktopPet-macOS", isDirectory: true)
    }()
    var now: Float { Float(ProcessInfo.processInfo.systemUptime - start) }
    func number(_ key: String) -> Double { (settings[key] as? NSNumber)?.doubleValue ?? 0 }
    func flag(_ key: String) -> Bool { settings[key] as? Bool ?? false }
    func loadSettings() {
        guard let ptr = pet_settings(core), let data = String(cString: ptr).data(using: .utf8),
              let dict = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        settings = dict
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        do { try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true) }
        catch { alert("无法创建设置目录：\(error.localizedDescription)"); NSApp.terminate(nil); return }
        core = dataDir.path.withCString { pet_create($0) }
        guard core != nil else { alert("无法读取设置。请检查设置目录的访问权限：\n\(dataDir.path)"); NSApp.terminate(nil); return }
        loadSettings()
        let width = 360 * number("scale"), height = 420 * number("scale")
        let screen = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        var origin = NSPoint(x: screen.maxX - width - 40, y: screen.minY + 30)
        if let pos = settings["position"] as? [String: Any], let x = pos["x"] as? Double, let y = pos["y"] as? Double {
            let saved = NSRect(x: x, y: y, width: width, height: height)
            if NSScreen.screens.contains(where: { $0.visibleFrame.intersects(saved.insetBy(dx: 50, dy: 50)) }) { origin = saved.origin }
        }
        pet = PetPanel(contentRect: NSRect(origin: origin, size: NSSize(width: width, height: height)),
                       styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        pet.title = "Minecraft桌宠 2.4.10 · macOS 修订 3"
        pet.isOpaque = false; pet.backgroundColor = .clear; pet.hasShadow = false
        pet.hidesOnDeactivate = false; pet.isReleasedWhenClosed = false
        pet.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        view = PetView(frame: NSRect(origin: .zero, size: pet.frame.size)); view.owner = self
        view.autoresizingMask = [.width, .height]; pet.contentView = view
        status = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = status.button {
            button.image = NSImage(systemSymbolName: "cube.fill", accessibilityDescription: "Minecraft桌宠")
            button.toolTip = "Minecraft桌宠 v2.4.10 · 点击打开菜单"
        }
        let menu = NSMenu(); menu.delegate = self; status.menu = menu; buildMenu(menu)
        mouseMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] _ in
            self?.handleMousePress()
        }
        localMouseMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] event in
            self?.handleMousePress(); return event
        }
        input.onKey = { [weak self] key, down in
            guard let self, self.core != nil else { return }
            pet_key(self.core, key, down, self.now); self.wake()
        }
        input.onReset = { [weak self] in
            guard let self, self.core != nil else { return }
            pet_clear_keys(self.core); self.wake()
        }
        input.onStateChange = { [weak self] in self?.updatePermission() }
        if !smoke {
            configureInput()
            let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in
                self?.input.refresh(); self?.updatePermission()
            }
            RunLoop.main.add(timer, forMode: .common); permissionTimer = timer
        }
        for name in [NSWorkspace.willSleepNotification, NSWorkspace.sessionDidResignActiveNotification] {
            sleepObservers.append(NSWorkspace.shared.notificationCenter.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                guard let self else { return }; self.input.resetState()
            })
        }
        applyWindow(); restartTimer(); wake(); tick(); pet.orderFrontRegardless()
        if !smoke && (CommandLine.arguments.contains("--show-settings") || (flag("keyboardEnabled") && !input.running) || (flag("hypQuickChat") && !input.quickChatReady)) {
            showSettings()
            if CommandLine.arguments.contains("--show-settings") { controlTabs?.selectTabViewItem(at: 1) }
        }
        if smoke {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                self.showSettings()
                if CommandLine.arguments.contains("--self-test-ui") { self.runUIRegression() }
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { self.writeSmokeReport() }
            DispatchQueue.main.asyncAfter(deadline: .now() + 2.0) { NSApp.terminate(nil) }
        }
    }
    func buildMenu(_ menu: NSMenu) {
        menu.removeAllItems()
        func item(_ title: String, _ action: Selector, _ state: Bool? = nil, _ key: String = "") {
            let i = NSMenuItem(title: title, action: action, keyEquivalent: key); i.target = self
            if let state { i.state = state ? .on : .off }; menu.addItem(i)
        }
        item("打开控制台…", #selector(showSettings), nil, ",")
        item("申请输入监控…", #selector(grantInput))
        item("申请辅助功能（快速聊天）…", #selector(grantAccessibility))
        item("hyp 快速聊天：# → /p chat ", #selector(toggleQuickChat), flag("hypQuickChat"))
        menu.addItem(.separator())
        item("完全鼠标穿透", #selector(toggleClickThrough), flag("clickThrough"))
        item("始终置顶", #selector(toggleTop), flag("alwaysOnTop"))
        item(hidden ? "显示桌宠" : "隐藏桌宠", #selector(toggleHidden))
        item("重新居中", #selector(recenter))
        menu.addItem(.separator())
        item("导入 PNG 皮肤…", #selector(importSkin))
        item("退出 Minecraft桌宠", #selector(quit), nil, "q")
    }
    func menuWillOpen(_ menu: NSMenu) { buildMenu(menu) }
    @objc func toggleQuickChat() {
        settings["hypQuickChat"] = !flag("hypQuickChat"); changed(); save()
        configureInput()
        if flag("hypQuickChat") && !input.accessibilityAllowed { grantAccessibility() }
        panel = closePanel(); showSettings()
    }
    @objc func toggleClickThrough() { settings["clickThrough"] = !flag("clickThrough"); changed(); panel = closePanel(); showSettings() }
    @objc func toggleTop() { settings["alwaysOnTop"] = !flag("alwaysOnTop"); changed() }
    @objc func toggleHidden() { hidden.toggle(); hidden ? pet.orderOut(nil) : pet.orderFrontRegardless(); wake() }
    @objc func recenter() {
        let s = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        pet.setFrameOrigin(NSPoint(x: s.midX - pet.frame.width/2, y: s.midY - pet.frame.height/2))
        savePosition(); wake()
    }
    @objc func quit() { NSApp.terminate(nil) }
    func handleMousePress() {
        guard core != nil else { return }
        mousePresses += 1; pet_click(core, now); wake()
    }
    func wake() { awakeUntil = now + 0.85 }
    func applyWindow() {
        pet.ignoresMouseEvents = flag("clickThrough")
        pet.level = flag("alwaysOnTop") ? .floating : .normal
        let size = NSSize(width: 360 * number("scale"), height: 420 * number("scale"))
        pet.setContentSize(size)
    }
    func restartTimer() {
        timer?.invalidate()
        let t = Timer(timeInterval: 1.0 / max(30, number("fps")), repeats: true) { [weak self] _ in self?.tick() }
        t.tolerance = 0.001; RunLoop.main.add(t, forMode: .common); timer = t
    }
    func changed() {
        previewSettings()
        applyWindow(); restartTimer(); wake()
        saveTimer?.invalidate()
        saveTimer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: false) { [weak self] _ in self?.save() }
    }
    func previewSettings() {
        guard let data = try? JSONSerialization.data(withJSONObject: settings),
              let json = String(data: data, encoding: .utf8) else { return }
        if !json.withCString({ pet_preview(core, $0) }) { showCoreError() }
    }
    func save() {
        saveTimer?.invalidate(); saveTimer = nil
        guard core != nil, let data = try? JSONSerialization.data(withJSONObject: settings),
              let json = String(data: data, encoding: .utf8) else { return }
        if !json.withCString({ pet_apply(core, $0) }) { showCoreError(); loadSettings(); applyWindow() }
    }
    func savePosition() {
        settings["position"] = ["x": pet.frame.origin.x, "y": pet.frame.origin.y]
        save()
    }
    func tick() {
        guard core != nil else { return }
        let clock = ProcessInfo.processInfo.systemUptime
        let elapsed = min(0.1, max(0, clock-lastTick))
        if clock - lastTick > 1 { input.resetState(); wake() }; lastTick = clock
        if !smoke { input.recoverReleasedKeys() }
        let cursor = NSEvent.mouseLocation
        let shift = CGEventSource.flagsState(.combinedSessionState).contains(.maskShift)
        if cursor != lastCursor && flag("trackMouse") { awakeUntil = max(awakeUntil, now+0.05) }
        if shift != lastShift && flag("keyboardEnabled") { wake() }
        lastCursor = cursor; lastShift = shift
        if !hidden && now <= awakeUntil {
            let r = pet.frame
            let x = Float(cursor.x - r.minX), y = Float(r.maxY - cursor.y)
            if let bytes = pet_render(core, x, y, Float(r.width), Float(r.height), shift, now, min(0.05, max(0.001, now-lastDraw))) {
                let data = Data(bytes: bytes, count: 450 * 525 * 4) as CFData
                if let provider = CGDataProvider(data: data) {
                    view.bitmap = CGImage(width: 450, height: 525, bitsPerComponent: 8, bitsPerPixel: 32,
                        bytesPerRow: 450 * 4, space: CGColorSpaceCreateDeviceRGB(),
                        bitmapInfo: CGBitmapInfo(rawValue: CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.premultipliedFirst.rawValue),
                        provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
                    view.needsDisplay = true
                }
            }
            lastDraw = now
        }
        let r = pet.frame
        let over = pet_hit(core, Float((cursor.x-r.minX)/r.width), Float((r.maxY-cursor.y)/r.height))
        let target: CGFloat = flag("clickThrough") && over ? 0.25 : 1
        let delta = target - pet.alphaValue
        if abs(delta) > 0.002 { pet.alphaValue += delta * min(1, elapsed / 0.045) }
        else { pet.alphaValue = target }
    }
    func configureInput() {
        if !smoke { input.configure(keyboard: flag("keyboardEnabled"), quickChat: flag("hypQuickChat")) }
        updatePermission()
    }
    @objc func grantInput() {
        NSApp.activate(ignoringOtherApps: true)
        _ = CGRequestListenEventAccess()
        configureInput()
        if !input.listeningAllowed {
            NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")!)
        }
    }
    @objc func grantAccessibility() {
        NSApp.activate(ignoringOtherApps: true)
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        _ = AXIsProcessTrustedWithOptions(options)
        configureInput()
        if !input.accessibilityAllowed {
            NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!)
        }
    }
    @objc func refreshPermissions() { input.refresh(force: true); updatePermission() }
    @objc func restartApplication() {
        savePosition()
        let configuration = NSWorkspace.OpenConfiguration(); configuration.createsNewApplicationInstance = true
        configuration.arguments = Array(CommandLine.arguments.dropFirst())
        NSWorkspace.shared.openApplication(at: Bundle.main.bundleURL, configuration: configuration) { _, error in
            DispatchQueue.main.async {
                if let error { self.alert(error.localizedDescription) }
                else { NSApp.terminate(nil) }
            }
        }
    }
    func applicationDidBecomeActive(_ notification: Notification) {
        if core != nil && !smoke { configureInput() }
    }
    func updatePermission() {
        let listen = input.listeningAllowed, access = input.accessibilityAllowed
        let keyboard = !flag("keyboardEnabled") ? "键盘互动已关闭" : (input.running ? "键盘互动已连接" : ((listen || access) ? "键盘权限已获准，连接待恢复" : "键盘互动等待授权"))
        let chat = !flag("hypQuickChat") ? "快速聊天已关闭" : (input.quickChatReady ? "快速聊天已连接" : (access ? "聊天权限已获准，连接待恢复" : "快速聊天等待辅助功能权限"))
        permissionSummary?.stringValue = "输入监控：\(listen ? "已授权" : "待授权")   辅助功能：\(access ? "已授权" : "待授权")\n\(keyboard) · \(chat)"
        inputRequestButton?.title = listen ? "输入监控已授权" : "申请输入监控"
        accessRequestButton?.title = access ? "辅助功能已授权" : "申请辅助功能"
        permissionLabel?.stringValue = "\(keyboard)。只用于角色动作，不保存或上传输入内容。"
        accessLabel?.stringValue = "\(chat)。点击上方按钮授权，返回后自动重连；系统要求重新启动时可点击“重启应用”。"
        inputHealthLabel?.stringValue = input.diagnosticSummary
        // Opt-in diagnostics contain counts and permissions, never key text/codes.
        if CommandLine.arguments.contains("--input-diagnostics"),
           let data = try? JSONSerialization.data(withJSONObject: input.diagnostic, options: [.prettyPrinted, .sortedKeys]) {
            try? data.write(to: dataDir.appendingPathComponent("input-status.json"), options: .atomic)
        }
    }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool { showSettings(); return true }
    func applicationWillTerminate(_ notification: Notification) {
        guard core != nil else { return }
        savePosition(); input.stop(); timer?.invalidate(); permissionTimer?.invalidate()
        if let monitor = mouseMonitor { NSEvent.removeMonitor(monitor) }
        if let monitor = localMouseMonitor { NSEvent.removeMonitor(monitor) }
        for token in sleepObservers { NSWorkspace.shared.notificationCenter.removeObserver(token) }
        pet_destroy(core); core = nil
    }
    func alert(_ text: String) {
        let a = NSAlert(); a.messageText = "Minecraft桌宠"; a.informativeText = text; a.runModal()
    }
    func showCoreError() { alert(pet_error(core).map { String(cString: $0) } ?? "操作失败。") }
    func closePanel() -> NSWindow? { panel?.close(); bindings.removeAll(); return nil }

    @objc func showSettings() {
        guard core != nil else { return }
        if let panel { panel.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true); updatePermission(); return }
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 660, height: 630),
                         styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        w.title = "Minecraft桌宠 v2.4.10 · macOS 修订 3"; w.isReleasedWhenClosed = false; w.delegate = self
        let root = NSView(frame: NSRect(x: 0, y: 0, width: 660, height: 630)); w.contentView = root
        let title = NSTextField(labelWithString: "MINECRAFT  桌宠")
        title.font = .boldSystemFont(ofSize: 22); title.frame = NSRect(x: 24, y: 574, width: 560, height: 32); root.addSubview(title)
        let subtitle = NSTextField(labelWithString: "原版 Rust 渲染器 · macOS 原生窗口 · 设置自动保存")
        subtitle.textColor = .secondaryLabelColor; subtitle.frame = NSRect(x: 24, y: 550, width: 600, height: 20); root.addSubview(subtitle)
        // Permission controls stay outside the scrolling pages and are always visible.
        let banner = NSBox(frame: NSRect(x: 20, y: 438, width: 620, height: 103)); banner.boxType = .custom
        banner.fillColor = .controlBackgroundColor; banner.borderColor = .separatorColor; banner.cornerRadius = 8
        root.addSubview(banner)
        let summary = NSTextField(wrappingLabelWithString: "")
        summary.font = .systemFont(ofSize: 12); summary.frame = NSRect(x: 34, y: 489, width: 590, height: 40)
        root.addSubview(summary); permissionSummary = summary
        func permissionButton(_ x: CGFloat, _ width: CGFloat, _ title: String, _ action: Selector) -> NSButton {
            let b = NSButton(title: title, target: self, action: action); b.bezelStyle = .rounded
            b.frame = NSRect(x: x, y: 449, width: width, height: 30); root.addSubview(b); return b
        }
        inputRequestButton = permissionButton(30, 156, "申请输入监控", #selector(grantInput))
        accessRequestButton = permissionButton(192, 156, "申请辅助功能", #selector(grantAccessibility))
        _ = permissionButton(354, 126, "重新检测", #selector(refreshPermissions))
        _ = permissionButton(486, 140, "重启应用", #selector(restartApplication))
        let tabs = NSTabView(frame: NSRect(x: 16, y: 42, width: 628, height: 385)); root.addSubview(tabs)
        controlTabs = tabs; tabs.delegate = self
        addGeneral(tabs); addInput(tabs); addHead(tabs); addPose(tabs); addSkins(tabs)
        let hint = NSTextField(labelWithString: "穿透时请用菜单栏的方块图标控制桌宠；关闭窗口后仍在后台运行。")
        hint.textColor = .secondaryLabelColor; hint.font = .systemFont(ofSize: 11)
        hint.frame = NSRect(x: 24, y: 14, width: 610, height: 18); root.addSubview(hint)
        tabs.layoutSubtreeIfNeeded(); scrollPageToTop(tabs.selectedTabViewItem)
        panel = w; w.center(); w.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true); updatePermission()
    }
    func windowWillClose(_ notification: Notification) { save() }
    func scrollPageToTop(_ item: NSTabViewItem?) {
        guard let scroll = item?.view as? NSScrollView, let document = scroll.documentView else { return }
        scroll.layoutSubtreeIfNeeded()
        scroll.contentView.scroll(to: NSPoint(x: 0, y: max(0, document.frame.height-scroll.contentView.bounds.height)))
        scroll.reflectScrolledClipView(scroll.contentView)
    }
    func tabView(_ tabView: NSTabView, didSelect tabViewItem: NSTabViewItem?) {
        scrollPageToTop(tabViewItem)
        DispatchQueue.main.async { [weak self, weak tabViewItem] in self?.scrollPageToTop(tabViewItem) }
    }
    func page(_ tabs: NSTabView, _ name: String, _ height: CGFloat = 600) -> NSView {
        let item = NSTabViewItem(identifier: name); item.label = name
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 600, height: 452)); scroll.hasVerticalScroller = true; scroll.autohidesScrollers = false; scroll.scrollerStyle = .legacy; scroll.drawsBackground = false
        let doc = NSView(frame: NSRect(x: 0, y: 0, width: 594, height: height)); scroll.documentView = doc
        item.view = scroll; tabs.addTabViewItem(item)
        scroll.contentView.scroll(to: NSPoint(x: 0, y: max(0, height - scroll.contentSize.height)))
        scroll.reflectScrolledClipView(scroll.contentView)
        return doc
    }
    func note(_ doc: NSView, _ y: CGFloat, _ text: String) {
        let label = NSTextField(wrappingLabelWithString: text); label.textColor = .secondaryLabelColor
        label.font = .systemFont(ofSize: 12); label.frame = NSRect(x: 22, y: y, width: 546, height: 45); doc.addSubview(label)
    }
    func button(_ doc: NSView, _ y: CGFloat, _ title: String, _ action: Selector) {
        let b = NSButton(title: title, target: self, action: action); b.bezelStyle = .rounded
        b.frame = NSRect(x: 20, y: y, width: 220, height: 30); doc.addSubview(b)
    }
    func check(_ doc: NSView, _ y: CGFloat, _ title: String, _ key: String) {
        let b = NSButton(checkboxWithTitle: title, target: self, action: #selector(controlChanged(_:)))
        b.state = flag(key) ? .on : .off; b.frame = NSRect(x: 24, y: y, width: 530, height: 26)
        bindings[ObjectIdentifier(b)] = Binding(key); doc.addSubview(b)
    }
    func popup(_ doc: NSView, _ y: CGFloat, _ title: String, _ key: String, _ names: [String], _ values: [Any]) {
        let label = NSTextField(labelWithString: title); label.frame = NSRect(x: 24, y: y+4, width: 168, height: 20); doc.addSubview(label)
        let b = NSPopUpButton(frame: NSRect(x: 196, y: y, width: 348, height: 28), pullsDown: false); b.addItems(withTitles: names)
        let current = String(describing: settings[key] ?? "")
        if let i = values.firstIndex(where: { String(describing: $0) == current }) { b.selectItem(at: i) }
        b.target = self; b.action = #selector(controlChanged(_:)); bindings[ObjectIdentifier(b)] = Binding(key, values); doc.addSubview(b)
    }
    func value(_ key: String) -> Double {
        let parts = key.split(separator: ".").map(String.init)
        if parts.count == 1 { return number(key) }
        if parts.count == 2, let index = Int(parts[1]), let list = settings[parts[0]] as? [Double] { return list[index] }
        if parts.count == 3, let dict = settings[parts[0]] as? [String: Any], let list = dict[parts[1]] as? [Double], let index = Int(parts[2]) { return list[index] }
        return 0
    }
    func setValue(_ key: String, _ value: Double) {
        let p = key.split(separator: ".").map(String.init)
        if p.count == 1 { settings[key] = value }
        else if p.count == 2, let i = Int(p[1]), var list = settings[p[0]] as? [Double] { list[i] = value; settings[p[0]] = list }
        else if p.count == 3, let i = Int(p[2]), var dict = settings[p[0]] as? [String: Any], var list = dict[p[1]] as? [Double] {
            list[i] = value; dict[p[1]] = list; settings[p[0]] = dict
        }
    }
    func slider(_ doc: NSView, _ y: CGFloat, _ title: String, _ key: String, _ min: Double, _ max: Double) {
        let label = NSTextField(labelWithString: title); label.frame = NSRect(x: 24, y: y+3, width: 166, height: 20); doc.addSubview(label)
        let v = value(key), out = NSTextField(labelWithString: String(format: "%.1f", v))
        out.alignment = .right; out.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        out.frame = NSRect(x: 492, y: y+3, width: 62, height: 20); doc.addSubview(out)
        let s = NSSlider(value: v, minValue: min, maxValue: max, target: self, action: #selector(controlChanged(_:)))
        s.frame = NSRect(x: 196, y: y, width: 282, height: 26); s.isContinuous = true
        bindings[ObjectIdentifier(s)] = Binding(key, [], out); doc.addSubview(s)
    }
    @objc func controlChanged(_ sender: NSControl) {
        guard let b = bindings[ObjectIdentifier(sender)] else { return }
        if let s = sender as? NSSlider { setValue(b.key, s.doubleValue); b.label?.stringValue = String(format: "%.1f", s.doubleValue) }
        else if let p = sender as? NSPopUpButton { settings[b.key] = b.choices[p.indexOfSelectedItem] }
        else if let c = sender as? NSButton { settings[b.key] = c.state == .on }
        changed()
        if b.key == "keyboardEnabled" || b.key == "hypQuickChat" {
            save(); configureInput()
            if b.key == "keyboardEnabled" && flag("keyboardEnabled") && !input.running { grantInput() }
            if b.key == "hypQuickChat" && flag("hypQuickChat") && !input.accessibilityAllowed { grantAccessibility() }
        }
    }
    func addInput(_ tabs: NSTabView) {
        let d = page(tabs, "输入与聊天", 480)
        check(d, 430, "启用 hyp 快速队伍聊天：# → /p chat ", "hypQuickChat")
        note(d, 369, "与 Windows 版一致：仅替换半角 #，末尾保留空格，不自动发送。长按只触发一次；Command / Control 快捷键保持原行为。")
        let label = NSTextField(wrappingLabelWithString: "")
        label.frame = NSRect(x: 24, y: 312, width: 540, height: 48); label.font = .systemFont(ofSize: 12); label.textColor = .secondaryLabelColor
        d.addSubview(label); accessLabel = label
        let field = NSTextField(frame: NSRect(x: 24, y: 266, width: 530, height: 28))
        field.placeholderString = "在这里输入 # 测试；应显示 /p chat（不会发送）"
        d.addSubview(field); diagnosticsField = field
        button(d, 215, "预览一次挥臂", #selector(previewSwing))
        let health = NSTextField(wrappingLabelWithString: input.diagnosticSummary)
        health.frame = NSRect(x: 24, y: 140, width: 540, height: 66)
        health.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        d.addSubview(health); inputHealthLabel = health
        note(d, 80, "辅助功能用于拦截井号并插入前缀。功能开启时对当前输入窗口生效；不使用剪贴板，不模拟松开物理修饰键。")
        note(d, 18, "更新后权限可能需要重新授权：在系统设置移除旧的桌宠条目，重新添加当前应用并打开开关，再重启。若提示安全输入，请退出密码输入框。")
    }
    @objc func previewSwing() {
        // The local mouse monitor is the single click source; keyboard activation needs a click.
        if NSApp.currentEvent?.type != .leftMouseUp { handleMousePress() }
        wake()
    }

    func addGeneral(_ tabs: NSTabView) {
        let d = page(tabs, "基本", 640)
        popup(d, 592, "姿态", "posture", ["站立", "坐下", "自定义"], ["standing", "sitting", "custom"])
        popup(d, 552, "手臂模型", "armModel", ["经典 · 4 像素", "纤细 · 3 像素"], ["classic", "slim"])
        popup(d, 512, "帧率上限", "fps", ["30", "60", "90", "120", "144", "165", "240"], [30,60,90,120,144,165,240])
        slider(d, 470, "桌宠缩放", "scale", 0.65, 1.8)
        check(d, 430, "始终置顶", "alwaysOnTop")
        check(d, 394, "完全鼠标穿透（悬停淡化）", "clickThrough")
        check(d, 358, "头部跟随鼠标", "trackMouse")
        check(d, 322, "显示皮肤外层", "outerLayer")
        check(d, 286, "全局键盘互动 / Shift 潜行", "keyboardEnabled")
        popup(d, 244, "打字动作", "typingMode", ["按键位置精准敲击", "双手交替"], ["precise", "alternating"])
        popup(d, 204, "点击挥动主手", "mainHand", ["右手", "左手"], ["right", "left"])
        slider(d, 164, "打字下压幅度 %", "typingSquash", 0, 30)
        let p = NSTextField(wrappingLabelWithString: ""); p.frame = NSRect(x: 24, y: 100, width: 540, height: 42); p.font = .systemFont(ofSize: 12); p.textColor = .secondaryLabelColor
        d.addSubview(p); permissionLabel = p
        button(d, 58, "立即申请输入监控…", #selector(grantInput))
        note(d, 4, "取消穿透后可拖动桌宠；双击或右键打开控制台。静止时暂停渲染。")
    }
    func addHead(_ tabs: NSTabView) {
        let d = page(tabs, "视角", 480)
        let items: [(String,String,Double,Double)] = [
            ("水平灵敏度 %","headSensitivityX",0,300),("垂直灵敏度 %","headSensitivityY",0,300),
            ("水平范围 °","headRangeX",0,85),("垂直范围 °","headRangeY",0,85),
            ("水平中心 °","headCenterX",-60,60),("垂直中心 °","headCenterY",-60,60),
            ("整体旋转 X °","modelRotation.0",-180,180),("整体旋转 Y °","modelRotation.1",-180,180),
            ("整体旋转 Z °","modelRotation.2",-180,180)]
        for (i,v) in items.enumerated() { slider(d, CGFloat(432-i*43), v.0, v.1, v.2, v.3) }
        note(d, 8, "头部跟随以全局鼠标位置为准，支持多个显示器。")
    }
    func addPose(_ tabs: NSTabView) {
        let d = page(tabs, "自定义姿态", 1770)
        popup(d, 1720, "自定义基础姿态", "customBase", ["站姿", "坐姿"], ["standing", "sitting"])
        note(d, 1660, "在“基本”页将姿态设为“自定义”即可使用。分别保存站姿与坐姿；以下为各部位 X / Y / Z 旋转角度。")
        let rows = [("head","头部"),("body","身体"),("leftArm","左臂"),("rightArm","右臂"),("leftLeg","左腿"),("rightLeg","右腿")]
        var y: CGFloat = 1610
        for (key, title) in [("customPose", "自定义站姿"),("customSittingPose", "自定义坐姿")] {
            let l = NSTextField(labelWithString: title); l.font = .boldSystemFont(ofSize: 14); l.frame = NSRect(x: 24, y: y, width: 540, height: 24); d.addSubview(l); y -= 38
            for (part,name) in rows {
                for axis in 0..<3 { slider(d, y, "\(name) \(["X","Y","Z"][axis]) °", "\(key).\(part).\(axis)", -180, 180); y -= 39 }
            }
            y -= 22
        }
    }
    func addSkins(_ tabs: NSTabView) {
        let d = page(tabs, "皮肤与关于", 520)
        note(d, 452, "当前皮肤：\(settings["skinName"] as? String ?? "内置")\n支持 64×64、64×32 及等比例高清 PNG（最大 1024×1024、8 MB）。")
        button(d, 404, "导入本地 PNG 皮肤…", #selector(importSkin))
        let f = NSTextField(frame: NSRect(x: 24, y: 350, width: 340, height: 26)); f.placeholderString = "Java 版用户名或 UUID"; d.addSubview(f); onlineField = f
        let b = NSButton(title: "获取正版皮肤", target: self, action: #selector(downloadSkin)); b.bezelStyle = .rounded; b.frame = NSRect(x: 374, y: 347, width: 175, height: 30); d.addSubview(b)
        let s = NSTextField(wrappingLabelWithString: "仅主动获取时连接 Mojang 官方资料与纹理服务。")
        s.frame = NSRect(x: 24, y: 292, width: 530, height: 44); s.textColor = .secondaryLabelColor; d.addSubview(s); onlineStatus = s
        button(d, 240, "打开设置文件夹", #selector(openDataFolder))
        button(d, 198, "恢复默认设置和内置皮肤", #selector(resetSettings))
        note(d, 120, "Minecraft桌宠 v2.4.10 · macOS 修订 3\n原版渲染与动画 · 输入权限引导 · hyp 快速聊天")
        note(d, 56, "独立粉丝作品，与 Mojang Studios / Microsoft 无关联。\n原项目 MIT 许可；项目与依赖许可随应用附带。")
        button(d, 12, "打开项目主页", #selector(openProject))
    }
    @objc func importSkin() {
        let p = NSOpenPanel(); p.allowedContentTypes = [.png]; p.allowsMultipleSelection = false; p.canChooseDirectories = false
        NSApp.activate(ignoringOtherApps: true)
        if p.runModal() == .OK, let url = p.url {
            downloadID = UUID()
            save()
            if !url.path.withCString({ pet_import(core, $0) }) { showCoreError(); return }
            loadSettings(); wake(); panel = closePanel(); showSettings()
        }
    }
    var downloadID = UUID()
    @objc func downloadSkin() {
        guard let input = onlineField?.stringValue.trimmingCharacters(in: .whitespacesAndNewlines), !input.isEmpty else { return }
        let compact = input.replacingOccurrences(of: "-", with: "")
        let isUUID = compact.count == 32 && compact.allSatisfy { $0.isHexDigit }
        let isName = input.count <= 16 && input.unicodeScalars.allSatisfy { CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_").contains($0) }
        guard isUUID || isName else { alert("请输入有效的 Java 版用户名或 UUID。"); return }
        let requestID = UUID(); downloadID = requestID
        onlineStatus?.stringValue = "正在连接官方皮肤服务…"
        Task { @MainActor in
            do {
                let uuid: String
                if isUUID { uuid = compact.lowercased() }
                else {
                    let profile = try await fetchJSON("https://api.mojang.com/users/profiles/minecraft/\(input)")
                    guard let id = profile["id"] as? String, id.count == 32, id.allSatisfy({ $0.isHexDigit }) else { throw PetError("没有找到这个 Java 版账号。") }
                    uuid = id
                }
                let profile = try await fetchJSON("https://sessionserver.mojang.com/session/minecraft/profile/\(uuid)")
                guard let props = profile["properties"] as? [[String: Any]], let encoded = props.first(where: { $0["name"] as? String == "textures" })?["value"] as? String,
                      let decoded = Data(base64Encoded: encoded), let payload = try JSONSerialization.jsonObject(with: decoded) as? [String: Any],
                      let textures = payload["textures"] as? [String: Any], let skin = textures["SKIN"] as? [String: Any], let address = skin["url"] as? String,
                      let url = URL(string: address), url.host == "textures.minecraft.net", url.path.hasPrefix("/texture/") else { throw PetError("该账号没有可用的官方皮肤。") }
                let hash = String(url.path.dropFirst("/texture/".count))
                guard !hash.isEmpty, hash.count <= 128, hash.allSatisfy({ $0.isHexDigit }) else { throw PetError("官方皮肤地址无效。") }
                let bytes = try await fetchData("https://textures.minecraft.net/texture/\(hash)", limit: 8*1024*1024)
                guard requestID == downloadID else { return }
                let tmp = dataDir.appendingPathComponent("download-\(requestID).png")
                try bytes.write(to: tmp, options: .atomic)
                defer { try? FileManager.default.removeItem(at: tmp) }
                save()
                guard tmp.path.withCString({ pet_import(core, $0) }) else { throw PetError(pet_error(core).map { String(cString: $0) } ?? "皮肤无法识别。") }
                loadSettings()
                settings["skinName"] = profile["name"] as? String ?? input
                settings["armModel"] = ((skin["metadata"] as? [String: Any])?["model"] as? String == "slim") ? "slim" : "classic"
                save(); wake(); onlineStatus?.stringValue = "已载入：\(settings["skinName"] as? String ?? input)"
            } catch { if requestID == downloadID { onlineStatus?.stringValue = error.localizedDescription } }
        }
    }
    func fetchData(_ address: String, limit: Int) async throws -> Data {
        var request = URLRequest(url: URL(string: address)!); request.timeoutInterval = 25
        request.setValue("MinecraftDesktopPet/2.4.10-macOS", forHTTPHeaderField: "User-Agent")
        // A byte stream enforces the response limit before retaining a large body.
        let (stream, response) = try await URLSession.shared.bytes(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else { throw PetError("官方服务返回错误，请检查用户名或稍后重试。") }
        guard response.url?.scheme == "https", response.url?.host == request.url?.host else { throw PetError("官方服务返回了意外的跳转。") }
        if response.expectedContentLength > Int64(limit) { throw PetError("官方服务返回的文件过大。") }
        var data = Data(); let deadline = Date().addingTimeInterval(30)
        for try await byte in stream {
            guard data.count < limit, Date() < deadline else { throw PetError("下载超时或文件过大。") }
            data.append(byte)
        }
        return data
    }
    func fetchJSON(_ url: String) async throws -> [String: Any] {
        let data = try await fetchData(url, limit: 128*1024)
        guard let dict = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw PetError("官方服务返回的数据无法识别。") }
        return dict
    }
    @objc func openDataFolder() { NSWorkspace.shared.open(dataDir) }
    @objc func openProject() { NSWorkspace.shared.open(URL(string: "https://github.com/DLmember/Minecraft-Desktop-Pet")!) }
    @objc func resetSettings() {
        downloadID = UUID(); saveTimer?.invalidate()
        if !pet_reset(core) { showCoreError(); return }
        loadSettings(); applyWindow(); restartTimer(); configureInput(); recenter(); wake(); panel = closePanel(); showSettings()
    }
    func runUIRegression() {
        let saved = settings
        settings["clickThrough"] = false; settings["trackMouse"] = false; settings["hypQuickChat"] = true
        changed(); save()
        let before = mousePresses
        let point = NSPoint(x: pet.frame.width/2, y: pet.frame.height/2)
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp, .otherMouseDown, .otherMouseUp] {
            if let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: pet.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 0.5) { NSApp.sendEvent(event) }
        }
        precondition(mousePresses == before+2, "Each physical click must dispatch exactly one swing")
        loadSettings(); precondition(flag("hypQuickChat"), "Quick chat option must persist")
        precondition(inputRequestButton?.superview === panel?.contentView, "Permission request must stay outside scroll view")
        precondition(controlTabs?.numberOfTabViewItems == 5)
        controlTabs?.selectTabViewItem(at: 1)
        if let field = diagnosticsField, let window = panel {
            window.makeFirstResponder(field)
            for event in QuickChat.replacementEvents()! {
                if let nsEvent = NSEvent(cgEvent: event) { NSApp.sendEvent(nsEvent) }
            }
            precondition(field.stringValue == QuickChat.text, "Injected Unicode events must insert exact text in AppKit")
            field.stringValue = ""
        }
        settings = saved; changed(); save()
        print("UI regression: click de-duplication, nonblocking mouse-up, quick-chat persistence, AppKit text insertion and fixed permission controls passed")
    }
    func writeSmokeReport() {
        guard let bitmap = view.bitmap else { return }
        if let content = panel?.contentView, let rep = content.bitmapImageRepForCachingDisplay(in: content.bounds) {
            content.cacheDisplay(in: content.bounds, to: rep)
            if let png = rep.representation(using: .png, properties: [:]) { try? png.write(to: dataDir.appendingPathComponent("smoke-settings.png")) }
        }
        let rep = NSBitmapImageRep(cgImage: bitmap)
        if let png = rep.representation(using: .png, properties: [:]) { try? png.write(to: dataDir.appendingPathComponent("smoke-frame.png")) }
        let report: [String: Any] = ["version":"2.4.10-mac.3", "width":bitmap.width, "height":bitmap.height,
            "windowVisible":pet.isVisible, "settingsWindowVisible":panel?.isVisible ?? false,
            "clickThrough":pet.ignoresMouseEvents, "settings":settings,
            "permissionButtonsVisible":inputRequestButton?.window != nil && accessRequestButton?.window != nil,
            "inputMonitoringGranted":input.listeningAllowed, "accessibilityGranted":input.accessibilityAllowed,
            "quickChatReady":input.quickChatReady, "mousePresses":mousePresses]
        if let data = try? JSONSerialization.data(withJSONObject: report, options: .prettyPrinted) { try? data.write(to: dataDir.appendingPathComponent("smoke-report.json")) }
    }
}
struct PetError: LocalizedError {
    let text: String
    init(_ text: String) { self.text = text }
    var errorDescription: String? { text }
}
if CommandLine.arguments.contains("--self-test-input") { InputSelfTests.run(); exit(0) }
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
