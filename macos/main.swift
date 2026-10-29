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
/// A provider retains its immutable frame until CoreGraphics finishes with it.
/// Reuse only unreferenced slots; a slow compositor never grows the buffer pool.
final class PixelBuffer {
    let pointer = UnsafeMutableRawPointer.allocate(byteCount: 450 * 525 * 4, alignment: 64)
    deinit { pointer.deallocate() }
}
final class PetView: NSView {
    weak var owner: AppDelegate?
    private var buffers = [PixelBuffer(), PixelBuffer()]
    private let colorSpace = CGColorSpaceCreateDeviceRGB()
    var bitmap: CGImage?
    private var dragOffset: NSPoint?
    private var didDrag = false
    override var isFlipped: Bool { true }
    func updatePixels(_ pixels: UnsafePointer<UInt8>) {
        for i in buffers.indices where isKnownUniquelyReferenced(&buffers[i]) {
            let buffer = buffers[i]
            memcpy(buffer.pointer, pixels, 450 * 525 * 4)
            let retained = Unmanaged.passRetained(buffer).toOpaque()
            guard let provider = CGDataProvider(dataInfo: retained, data: buffer.pointer, size: 450 * 525 * 4,
                releaseData: { info, _, _ in if let info { Unmanaged<PixelBuffer>.fromOpaque(info).release() } }) else {
                Unmanaged<PixelBuffer>.fromOpaque(retained).release(); return
            }
            bitmap = CGImage(width: 450, height: 525, bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: 450 * 4, space: colorSpace,
                bitmapInfo: CGBitmapInfo(rawValue: CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.premultipliedFirst.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
            needsDisplay = true; return
        }
    }
    override func draw(_ dirtyRect: NSRect) {
        guard let bitmap, let ctx = NSGraphicsContext.current?.cgContext else { return }
        ctx.clear(bounds); ctx.saveGState(); ctx.interpolationQuality = .none
        ctx.translateBy(x:0,y:bounds.height); ctx.scaleBy(x:1,y:-1)
        ctx.draw(bitmap,in:bounds); ctx.restoreGState()
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
    override func rightMouseDown(with event: NSEvent) { owner?.showPetMenu(event, in: self) }
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
    var startupPending = true
    var restartPending = false
    let music = MusicMonitor()
    var permissionPanel: NSWindow?
    var updateInFlight = false
    var glyphsReady = false
    var renderedFrames = 0
    var timerActive = true
    var selectedConsolePage = 0
    var uiReady = false
    weak var musicToggle: NSButton?
    var pageButtons: [NSButton] = []
    var saveStatus: NSTextField?
    var musicStatus = "音乐音符已关闭。"
    var startupStatus: NSTextField?
    let loginDirectory = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/LaunchAgents", isDirectory: true)
    lazy var network = URLSession(configuration: .ephemeral, delegate: OfficialSessionDelegate(), delegateQueue: nil)
    var core: OpaquePointer!
    var settings: [String: Any] = [:]
    var pet: PetPanel!
    var view: PetView!
    var status: NSStatusItem!
    var panel: NSWindow?
    /// Reuse the native console window after closing it. Rebuilding only its
    /// content prevents AppKit from accumulating window and tab allocations
    /// during repeated open/close cycles.
    var consoleWindow: NSWindow?
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
    var skinSummary: NSTextField?
    var lastCursor = NSPoint(x: -9999, y: -9999)
    var lastShift = false
    var lastDraw: Float = 0
    var awakeUntil: Float = 1
    var hidden = false
    var lastTick = ProcessInfo.processInfo.systemUptime
    let start = ProcessInfo.processInfo.systemUptime
    let smoke = CommandLine.arguments.contains("--smoke-test") || CommandLine.arguments.contains("--self-test-ui") || CommandLine.arguments.contains("--memory-test") || CommandLine.arguments.contains("--self-test-startup") || CommandLine.arguments.contains("--self-test-restart")
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
        pet.title = "Minecraft桌宠 \(appVersion) · macOS"
        pet.isOpaque = false; pet.backgroundColor = .clear; pet.hasShadow = false
        pet.hidesOnDeactivate = false; pet.isReleasedWhenClosed = false
        pet.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        view = PetView(frame: NSRect(origin: .zero, size: pet.frame.size)); view.owner = self
        view.autoresizingMask = [.width, .height]; pet.contentView = view
        status = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = status.button {
            let icon = NSImage(size: NSSize(width: 18, height: 18), flipped: false) { _ in
                NSColor.labelColor.setStroke()
                let path = NSBezierPath()
                path.move(to:NSPoint(x:2,y:5)); path.line(to:NSPoint(x:9,y:1)); path.line(to:NSPoint(x:16,y:5))
                path.line(to:NSPoint(x:16,y:13)); path.line(to:NSPoint(x:9,y:17)); path.line(to:NSPoint(x:2,y:13)); path.close()
                path.move(to:NSPoint(x:2,y:13)); path.line(to:NSPoint(x:9,y:9)); path.line(to:NSPoint(x:16,y:13))
                path.move(to:NSPoint(x:9,y:9)); path.line(to:NSPoint(x:9,y:1)); path.lineWidth=1.4; path.stroke()
                return true
            }
            icon.isTemplate = true; button.image = icon
            button.toolTip = "Minecraft桌宠 v\(appVersion) · 点击打开菜单"
        }
        let menu = NSMenu(); menu.delegate = self; status.menu = menu; buildMenu(menu)
        if !smoke {
            do {
                if flag("autostart") { try syncLoginStartup() }

            } catch { alert(error.localizedDescription) }
        }
        let delay = smoke ? 0 : LoginStartup.delay(arguments: CommandLine.arguments, enabled: flag("autostart"), seconds: Int(number("autostartDelay")))
        if delay > 0 {
            status.button?.toolTip = "Minecraft桌宠：登录启动等待 \(delay) 秒；打开控制台可立即启动"
            DispatchQueue.main.asyncAfter(deadline: .now() + .seconds(delay)) { [weak self] in self?.finishStartup() }
        } else { finishStartup() }
    }
    func finishStartup() {
        guard startupPending else { return }
        startupPending = false
        status.button?.toolTip = "Minecraft桌宠 v\(appVersion) · 点击打开菜单"
        mouseMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] event in
            self?.handleMousePress(event)
        }
        localMouseMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] event in
            self?.handleMousePress(event); return event
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
        music.onChange = { [weak self] playing, message in
            guard let self, self.core != nil else { return }
            pet_music(self.core, playing); self.wake()
            self.musicStatus = message; self.refreshMusicLabel()
        }
        configureMusic()
        if flag("typingLetters") { prepareGlyphs() }
        applyWindow(); restartTimer(); wake(); tick(); pet.orderFrontRegardless()
        if (!smoke || CommandLine.arguments.contains("--self-test-startup")) && LaunchMode.shouldShowSettings(arguments: CommandLine.arguments,
            keyboardEnabled: flag("keyboardEnabled"), keyboardRunning: input.running,
            quickChatEnabled: flag("hypQuickChat"), quickChatReady: input.quickChatReady) {
            showSettings()

        }
        if CommandLine.arguments.contains("--self-test-restart") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { self.runRestartRegression() }
        } else if CommandLine.arguments.contains("--memory-test") {
            startMemoryExercise()
        } else if CommandLine.arguments.contains("--self-test-startup") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
                precondition(CommandLine.arguments.contains("--autostart"))
                precondition(self.pet.isVisible && self.panel == nil && self.permissionPanel == nil,
                             "Login launch must show only the pet, even with no input connection")
                self.writeSmokeReport()
                print("Startup regression passed: pet visible, console and permission panels closed")
                NSApp.terminate(nil)
            }
        } else if smoke {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                self.showSettings()
                if let i = CommandLine.arguments.firstIndex(of: "--preview-page"), CommandLine.arguments.count > i + 1,
                   let page = Int(CommandLine.arguments[i + 1]), (0..<3).contains(page) { self.controlTabs?.selectTabViewItem(at: page) }
                if CommandLine.arguments.contains("--self-test-ui") { self.runUIRegression() }
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { self.writeSmokeReport() }
            let duration: Double
            if let i = CommandLine.arguments.firstIndex(of: "--smoke-seconds"), CommandLine.arguments.count > i + 1 {
                duration = min(60, max(2, Double(CommandLine.arguments[i + 1]) ?? 2))
            } else { duration = 2 }
            DispatchQueue.main.asyncAfter(deadline: .now() + duration) { NSApp.terminate(nil) }
        }
    }
    func buildMenu(_ menu: NSMenu) {
        menu.removeAllItems()
        func item(_ title: String, _ action: Selector, _ state: Bool? = nil, _ key: String = "") {
            let i = NSMenuItem(title: title, action: action, keyEquivalent: key); i.target = self
            if let state { i.state = state ? .on : .off }; menu.addItem(i)
        }
        item("打开控制台…", #selector(showSettings), nil, ",")
        if startupPending {
            item("退出 Minecraft桌宠", #selector(quit), nil, "q")
            return
        }
        item("申请输入监控…", #selector(grantInput))
        item("申请辅助功能（快速聊天）…", #selector(grantAccessibility))
        item("hyp 快速聊天：# → /p chat ", #selector(toggleQuickChat), flag("hypQuickChat"))
        menu.addItem(.separator())
        item("完全鼠标穿透", #selector(toggleClickThrough), flag("clickThrough"))
        item("始终置顶", #selector(toggleTop), flag("alwaysOnTop"))
        item(hidden ? "显示桌宠" : "隐藏桌宠", #selector(toggleHidden))
        item("重新居中", #selector(recenter))
        menu.addItem(.separator())
        item("喂食苹果", #selector(feedApple))
        item("导入 PNG 皮肤…", #selector(importSkin))
        item("退出 Minecraft桌宠", #selector(quit), nil, "q")
    }
    func menuWillOpen(_ menu: NSMenu) { buildMenu(menu) }
    @objc func toggleQuickChat() {
        settings["hypQuickChat"] = !flag("hypQuickChat"); changed(); save()
        configureInput()
        if flag("hypQuickChat") && !input.accessibilityAllowed { grantAccessibility() }
    }
    @objc func toggleClickThrough() { settings["clickThrough"] = !flag("clickThrough"); changed() }
    @objc func toggleTop() { settings["alwaysOnTop"] = !flag("alwaysOnTop"); changed() }
    @objc func toggleHidden() { hidden.toggle(); hidden ? pet.orderOut(nil) : pet.orderFrontRegardless(); wake() }
    @objc func recenter() {
        let s = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        pet.setFrameOrigin(NSPoint(x: s.midX - pet.frame.width/2, y: s.midY - pet.frame.height/2))
        savePosition(); wake()
    }
    @objc func quit() { NSApp.terminate(nil) }
    func handleMousePress(_ event: NSEvent? = nil) {
        guard core != nil, pet != nil, !hidden else { return }
        // Local console controls must not trigger pet interactions.
        if let window = event?.window, window !== pet { return }
        let cursor = NSEvent.mouseLocation, r = pet.frame
        let over = pet_hit(core, Float((cursor.x-r.minX)/r.width), Float((r.maxY-cursor.y)/r.height))
        mousePresses += 1
        pet_mouse(core, UInt32(clamping: event?.buttonNumber ?? 0), over, now)
        wake()
    }
    @objc func feedApple() { guard core != nil else { return }; pet_feed(core, now); wake() }
    func showPetMenu(_ event: NSEvent, in view: NSView) {
        let menu = NSMenu()
        for (title, action) in [("喂食苹果", #selector(feedApple)), ("打开控制台…", #selector(showSettings))] {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
            item.target = self; menu.addItem(item)
        }
        NSMenu.popUpContextMenu(menu, with: event, for: view)
    }
    func wake() {
        awakeUntil = now + 0.85
        if !timerActive { timerActive = true; restartTimer() }
    }
    func applyWindow() {
        pet.ignoresMouseEvents = flag("clickThrough")
        pet.level = flag("alwaysOnTop") ? .floating : .normal
        let size = NSSize(width: 360 * number("scale"), height: 420 * number("scale"))
        pet.setContentSize(size)
    }
    func restartTimer() {
        timer?.invalidate()
        let rate = timerActive ? max(30, number("fps")) : (hidden ? 2.0 : 30.0)
        let t = Timer(timeInterval: 1.0 / rate, repeats: true) { [weak self] _ in autoreleasepool { self?.tick() } }
        t.tolerance = 0.001; RunLoop.main.add(t, forMode: .common); timer = t
    }
    func changed() {
        previewSettings()
        applyWindow(); restartTimer(); wake()
        saveStatus?.stringValue = "待保存…"
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
        if !json.withCString({ pet_apply(core, $0) }) { saveStatus?.stringValue = "保存失败"; showCoreError(); loadSettings(); applyWindow() }
        else { saveStatus?.stringValue = "已保存" }
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
        let active = !hidden && (now <= awakeUntil || pet_needs_frames(core))
        if active != timerActive { timerActive = active; restartTimer() }
        if active {
            let r = pet.frame
            let x = Float(cursor.x - r.minX), y = Float(r.maxY - cursor.y)
            if let bytes = pet_render(core, x, y, Float(r.width), Float(r.height), shift, now, min(0.05, max(0.001, now-lastDraw))) {
                view.updatePixels(bytes); renderedFrames += 1
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
        guard !restartPending else { return }
        restartPending = true
        savePosition()
        // A permission sheet can prevent AppKit from terminating. Dismiss it
        // first, then relaunch only after this process has actually exited.
        closePermissions()
        let helper = Process()
        helper.executableURL = URL(fileURLWithPath: "/bin/sh")
        helper.arguments = ["-c", """
            parent="$1"
            shift
            attempts=0
            while /bin/kill -0 "$parent" 2>/dev/null; do
                attempts=$((attempts + 1))
                [ "$attempts" -lt 200 ] || exit 1
                /bin/sleep 0.1
            done
            exec /usr/bin/open -n -a "$@"
            """, "pet-restart", String(ProcessInfo.processInfo.processIdentifier),
            Bundle.main.bundleURL.path, "--args"] + Array(CommandLine.arguments.dropFirst()).filter { $0 != "--autostart" }
        helper.standardInput = FileHandle.nullDevice
        helper.standardOutput = FileHandle.nullDevice
        helper.standardError = FileHandle.nullDevice
        do { try helper.run() }
        catch {
            restartPending = false
            alert("无法重启应用：\(error.localizedDescription)")
            return
        }
        DispatchQueue.main.async { NSApp.terminate(nil) }
    }
    func applicationDidBecomeActive(_ notification: Notification) {
        if core != nil && !smoke && !startupPending { configureInput() }
    }
    func updatePermission() {
        let listen = input.listeningAllowed, access = input.accessibilityAllowed
        let keyboard = !flag("keyboardEnabled") ? "键盘互动已关闭" : (input.running ? "键盘互动已连接" : ((listen || access) ? "键盘权限已获准，连接待恢复" : "键盘互动等待授权"))
        let chat = !flag("hypQuickChat") ? "快速聊天已关闭" : (input.quickChatReady ? "快速聊天已连接" : (access ? "聊天权限已获准，连接待恢复" : "快速聊天等待辅助功能权限"))
        permissionSummary?.stringValue = "\(keyboard) · \(chat)"
        inputRequestButton?.title = listen ? "输入监控已授权" : "申请输入监控"
        accessRequestButton?.title = access ? "辅助功能已授权" : "申请辅助功能"
        permissionLabel?.stringValue = "\(keyboard)。只用于角色动作，不保存或上传输入内容。"
        accessLabel?.stringValue = "输入监控：\(listen ? "已授权" : "待授权") · 辅助功能：\(access ? "已授权" : "待授权")\n\(keyboard) · \(chat)"
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
        if pet != nil && !startupPending { savePosition() }; input.stop(); music.stop(); timer?.invalidate(); permissionTimer?.invalidate()
        if let monitor = mouseMonitor { NSEvent.removeMonitor(monitor) }
        if let monitor = localMouseMonitor { NSEvent.removeMonitor(monitor) }
        for token in sleepObservers { NSWorkspace.shared.notificationCenter.removeObserver(token) }
        pet_destroy(core); core = nil
    }
    func alert(_ text: String) {
        let a = NSAlert(); a.messageText = "Minecraft桌宠"; a.informativeText = text; a.runModal()
    }
    func showCoreError() { alert(pet_error(core).map { String(cString: $0) } ?? "操作失败。") }
    func closePanel() -> NSWindow? { panel?.close(); releaseConsole(); return nil }

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
                      let textures = payload["textures"] as? [String: Any], let skin = textures["SKIN"] as? [String: Any], let address = skin["url"] as? String else { throw PetError("该账号没有可用的官方皮肤。") }
                let bytes = try await fetchData(OfficialTexture.address(address), limit: 8*1024*1024)
                var capeBytes: Data?
                if let rawCape = textures["CAPE"] {
                    guard let cape = rawCape as? [String: Any], let address = cape["url"] as? String else { throw PetError("官方披风资料无效。") }
                    capeBytes = try await fetchData(OfficialTexture.address(address), limit: 2*1024*1024)
                }
                guard requestID == downloadID else { return }
                let tmp = dataDir.appendingPathComponent("download-\(requestID).png")
                try bytes.write(to: tmp, options: .atomic)
                let capeTmp = dataDir.appendingPathComponent("cape-download-\(requestID).png")
                defer {
                    try? FileManager.default.removeItem(at: tmp)
                    try? FileManager.default.removeItem(at: capeTmp)
                }
                if let capeBytes { try capeBytes.write(to: capeTmp, options: .atomic) }
                save()
                let name = profile["name"] as? String ?? input
                let model = ((skin["metadata"] as? [String: Any])?["model"] as? String == "slim") ? "slim" : "classic"
                let imported = tmp.path.withCString { skinPath in
                    name.withCString { namePtr in model.withCString { modelPtr in
                        if capeBytes != nil { return capeTmp.path.withCString { pet_import_profile(core, skinPath, $0, namePtr, modelPtr) } }
                        return pet_import_profile(core, skinPath, nil, namePtr, modelPtr)
                    } }
                }
                guard imported else { throw PetError(pet_error(core).map { String(cString: $0) } ?? "纹理无法识别。") }
                loadSettings(); updateSkinSummary(); wake()
                onlineStatus?.stringValue = "已载入：\(name) · \(capeBytes == nil ? "该账号未装备披风" : "皮肤与披风已保存")"
            } catch { if requestID == downloadID { onlineStatus?.stringValue = "获取失败，保留原皮肤与披风：\(error.localizedDescription)" } }
        }
    }
    func fetchData(_ address: String, limit: Int) async throws -> Data {
        var request = URLRequest(url: URL(string: address)!); request.timeoutInterval = 25
        request.setValue("MinecraftDesktopPet/\(appVersion)-macOS", forHTTPHeaderField: "User-Agent")
        // A byte stream enforces the response limit before retaining a large body.
        let (stream, response) = try await network.bytes(for: request)
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
        if !smoke && flag("autostart") {
            do { try LoginStartup.setEnabled(false, app: Bundle.main.bundleURL, data: dataDir, directory: loginDirectory) }
            catch { alert(error.localizedDescription); return }
        }
        if !pet_reset(core) { showCoreError(); return }
        loadSettings(); configureMusic(); applyWindow(); restartTimer(); configureInput(); recenter(); wake(); panel = closePanel(); showSettings()
    }
    func writeSmokeReport() {
        guard let bitmap = view.bitmap else { return }
        if let content = panel?.contentView, let rep = content.bitmapImageRepForCachingDisplay(in: content.bounds) {
            content.cacheDisplay(in: content.bounds, to: rep)
            if let png = rep.representation(using: .png, properties: [:]) { try? png.write(to: dataDir.appendingPathComponent("smoke-settings.png")) }
        }
        let rep = NSBitmapImageRep(cgImage: bitmap)
        if let png = rep.representation(using: .png, properties: [:]) { try? png.write(to: dataDir.appendingPathComponent("smoke-frame.png")) }
        let report: [String: Any] = ["version":appVersion, "width":bitmap.width, "height":bitmap.height,
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
if CommandLine.arguments.contains("--probe-audio") {
    let result = AudioOutputProbe.snapshot()
    print("Audio output: \(result.playing.map(String.init) ?? "unavailable"); source: \(result.source)")
    exit(0)
}
if CommandLine.arguments.contains("--self-test-input") { InputSelfTests.run(); exit(0) }
if CommandLine.arguments.contains(where: { ["--self-test-ui", "--smoke-test", "--memory-test", "--self-test-startup", "--self-test-restart"].contains($0) })
    && !CommandLine.arguments.contains("--data-dir") { fputs("UI tests require --data-dir\n", stderr); exit(2) }
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
