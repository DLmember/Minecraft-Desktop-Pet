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
    let pointer: UnsafeMutableRawPointer
    init(bytes: Int) { pointer = .allocate(byteCount: bytes, alignment: 64) }
    deinit { pointer.deallocate() }
}
final class PetView: NSView {
    weak var owner: AppDelegate?
    private var buffers: [PixelBuffer] = []
    private var pixelSize = NSSize.zero
    private let colorSpace = CGColorSpaceCreateDeviceRGB()
    var bitmap: CGImage?
    private var dragOffset: NSPoint?
    private var didDrag = false
    override var isFlipped: Bool { true }
    @discardableResult
    func updatePixels(_ pixels: UnsafePointer<UInt8>, width: Int, height: Int) -> Bool {
        let size = NSSize(width: width, height: height), bytes = width * height * 4
        if size != pixelSize {
            // Allow the window server to retain the displayed and previous frames
            // while the producer fills the next slot. The pool stays bounded.
            buffers.removeAll(keepingCapacity: true)
            pixelSize = size
        }
        let free = buffers.indices.first { isKnownUniquelyReferenced(&buffers[$0]) }
        if free == nil {
            guard buffers.count < 3 else { return false }
            buffers.append(PixelBuffer(bytes: bytes))
        }
        let i = free ?? (buffers.count - 1)
        let buffer = buffers[i]
        memcpy(buffer.pointer, pixels, bytes)
        let retained = Unmanaged.passRetained(buffer).toOpaque()
        guard let provider = CGDataProvider(dataInfo: retained, data: buffer.pointer, size: bytes,
            releaseData: { info, _, _ in if let info { Unmanaged<PixelBuffer>.fromOpaque(info).release() } }) else {
            Unmanaged<PixelBuffer>.fromOpaque(retained).release(); return false
        }
        guard let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: width * 4, space: colorSpace,
            bitmapInfo: CGBitmapInfo(rawValue: CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.premultipliedFirst.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else { return false }
        bitmap = image; needsDisplay = true; return true
    }
    func releaseSoftwareBuffers() { buffers.removeAll() }
    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        ctx.clear(bounds)
        guard let bitmap else { return }
        ctx.saveGState(); ctx.interpolationQuality = .none
        ctx.translateBy(x:0,y:bounds.height); ctx.scaleBy(x:1,y:-1)
        ctx.draw(bitmap,in:bounds); ctx.restoreGState()
    }
    override func mouseDown(with event: NSEvent) {
        guard let owner, let window else { return }
        if event.clickCount == 2 && owner.openConsoleOnDoubleClick() { return }
        // Local event monitor already dispatched exactly one swing.
        let cursor = NSEvent.mouseLocation
        dragOffset = NSPoint(x: cursor.x-window.frame.minX, y: cursor.y-window.frame.minY)
        didDrag = false
    }
    override func mouseDragged(with event: NSEvent) {
        guard let offset = dragOffset, let window, let owner else { return }
        // While a press may still become a long-press pat, keep the window still.
        // Moving past the hold tolerance ends it, then the drag continues from here.
        if owner.updateHold() { return }
        let cursor = NSEvent.mouseLocation
        window.setFrameOrigin(NSPoint(x: cursor.x-offset.x, y: cursor.y-offset.y))
        didDrag = true
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
    let consoleClient = CommandLine.arguments.contains("--settings-window")
    var consoleProcess: Process?
    var consoleChannel: ConsoleChannel?
    var consoleSent: [String: Any] = [:]
    var startupPending = true
    /// Launch arguments, with "--autostart" added for a System Settings login item.
    var launchArguments = CommandLine.arguments
    var startupDelay = 0
    var restartPending = false
    let music = MusicMonitor()
    var capePanel: NSPanel?
    var capeEntries: [CapeEntry] = []
    var capePage = 0
    var captureWindow: NSWindow?
    var captureView: CaptureView?
    var captureParked = false
    var permissionPanel: NSWindow?
    var permissionTint: NSView?
    var guidePanel: NSPanel?
    var guidePane: PrivacyPane?
    var guideStatus: NSTextField?
    var guideTimer: Timer?
    var updateInFlight = false
    var glyphsReady = false
    var renderedFrames = 0
    var metalFrames = 0
    var softwareFrames = 0
    private var rasterCleanup: DispatchWorkItem?
    var metal: MetalTestRenderer?
    var timerRate: Double = 0
    var drawRate: Double = 0
    var nextDraw: Double = 0
    var needsRedraw = true
    var holdPending = false
    var benchmarkCursor: NSPoint?
    var benchmarkProjectCPU: UInt64 = 0
    var benchmarkSubmitCPU: UInt64 = 0
    lazy var benchmarkBreakdown = cpuTest && CommandLine.arguments.contains("--metal-breakdown")
    private(set) var displayHz = 60.0
    private var displayClock: NSObject?
    private var idleGlobalMove: Any?
    private var idleLocalMove: Any?
    private var pointerAwakeUntil = 0.0
    private var displayTick = false
    private var consecutiveGPUFailures = 0
    var renderSize = NSSize(width: 450, height: 525)
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
    var keyboardColorField: NSTextField?
    var lockDoubleClickBox: NSButton?
    var onlineStatus: NSTextField?
    var skinSummary: NSTextField?
    var drawnGaze: NSPoint?
    var lastShift = false
    var lastDraw: Double = 0
    var awakeUntil: Double = 0
    var hidden = false
    var menuOpen = false
    let patSound = PatSound()
    var lastTick = ProcessInfo.processInfo.systemUptime
    let start = ProcessInfo.processInfo.systemUptime
    let cpuTest = CommandLine.arguments.contains("--cpu-test")
    let smoke = CommandLine.arguments.contains("--cpu-test") || CommandLine.arguments.contains("--smoke-test") || CommandLine.arguments.contains("--self-test-ui") || CommandLine.arguments.contains("--memory-test") || CommandLine.arguments.contains("--self-test-startup") || CommandLine.arguments.contains("--self-test-restart")
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
        consoleChannel?.send(["refresh": dict])
        metal?.invalidateTextures()
    }
    func applicationWillFinishLaunching(_ notification: Notification) {
        // The open-application event is current only while launching.
        let event = NSAppleEventManager.shared().currentAppleEvent
        let loginItem = event?.paramDescriptor(forKeyword: AEKeyword(keyAELaunchedAsLogInItem)) != nil
            || event?.paramDescriptor(forKeyword: AEKeyword(keyAEPropData))?.enumCodeValue == OSType(keyAELaunchedAsLogInItem)
        launchArguments = LaunchMode.loginArguments(CommandLine.arguments, launchedAsLoginItem: loginItem)
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        if consoleClient { startConsoleClient(); return }
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
        pet.acceptsMouseMovedEvents = true
        pet.delegate = self
        pet.title = "Minecraft桌宠 \(appVersion) · macOS"
        pet.isOpaque = false; pet.backgroundColor = .clear; pet.hasShadow = false
        pet.hidesOnDeactivate = false; pet.isReleasedWhenClosed = false
        pet.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        view = PetView(frame: NSRect(origin: .zero, size: pet.frame.size)); view.owner = self
        view.autoresizingMask = [.width, .height]; pet.contentView = view
        refreshDisplayRate()
        let testMetal = CommandLine.arguments.contains("--metal-test")
        if (!smoke || testMetal) && !CommandLine.arguments.contains("--software-renderer") {
            do {
                let args = CommandLine.arguments
                #if arch(arm64)
                let preferPool = !args.contains("--metal-drawable") && !args.contains("--metal-offscreen")
                #else
                let preferPool = false
                #endif
                let surface = args.contains("--metal-surface") || preferPool
                installRenderer(try MetalTestRenderer(
                    offscreen: args.contains("--metal-offscreen"),
                    trackPresentations: cpuTest && !args.contains("--metal-no-present-counter"),
                    useSurface: surface,
                    poolSurfaces: args.contains("--metal-pool-surfaces") || (preferPool && !args.contains("--metal-surface"))
                ))
            } catch {
                fputs("Metal initialization failed: \(error)\n", stderr)
                if testMetal { exit(1) }
            }
        }
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
        let delay = smoke ? 0 : LoginStartup.delay(arguments: launchArguments, enabled: flag("autostart"), seconds: Int(number("autostartDelay")))
        startupDelay = delay
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
            if down { self.prepareLettersIfNeeded() }
            pet_key(self.core, key, down, self.now); self.wake(interactive: true)
        }
        input.onReset = { [weak self] in
            guard let self, self.core != nil else { return }
            pet_clear_keys(self.core); self.wake(interactive: true)
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
        applyWindow(); tick(); pet.orderFrontRegardless()
        if (!smoke || CommandLine.arguments.contains("--self-test-startup")) && LaunchMode.shouldShowSettings(arguments: launchArguments,
            keyboardEnabled: flag("keyboardEnabled"), keyboardRunning: input.running,
            quickChatEnabled: flag("hypQuickChat"), quickChatReady: input.quickChatReady) {
            showSettings()

        }
        if CommandLine.arguments.contains("--self-test-restart") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { self.runRestartRegression() }
        } else if CommandLine.arguments.contains("--cpu-test") {
            startCPUExercise()
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
    @objc func toggleQuickChat() {
        settings["hypQuickChat"] = !flag("hypQuickChat"); changed(); save()
        configureInput()
        if flag("hypQuickChat") && !input.accessibilityAllowed { grantAccessibility() }
        refreshConsoleControls()
    }
    // Menu-bar toggles change settings directly; an open console only mirrors them.
    @objc func toggleClickThrough() { settings["clickThrough"] = !flag("clickThrough"); changed(); refreshConsoleControls() }
    @objc func toggleTop() { settings["alwaysOnTop"] = !flag("alwaysOnTop"); changed(); refreshConsoleControls() }
    @objc func toggleHidden() {
        hidden.toggle()
        if hidden { pet.orderOut(nil) }
        wake()
        tick()
        if !hidden { pet.orderFrontRegardless() }
    }
    @objc func recenter() {
        let s = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        pet.setFrameOrigin(NSPoint(x: s.midX - pet.frame.width/2, y: s.midY - pet.frame.height/2))
        savePosition(); wake()
    }
    @objc func quit() { NSApp.terminate(nil) }
    func handleMousePress(_ event: NSEvent? = nil) {
        guard !cpuTest else { return }
        guard core != nil, pet != nil, !hidden else { return }
        // Local console controls must not trigger pet interactions.
        if let window = event?.window, window !== pet { return }
        let cursor = NSEvent.mouseLocation, r = pet.frame
        let over = pet_hit(core, Float((cursor.x-r.minX)/r.width), Float((r.maxY-cursor.y)/r.height))
        if over && (event?.buttonNumber ?? 0) == 0 { holdPending = true }
        mousePresses += 1
        let meow = pet_mouse(core, UInt32(clamping: event?.buttonNumber ?? 0), over, Float(cursor.x), Float(cursor.y), now)
        if meow >= 0 && !smoke { patSound.play(Int(meow), volume: number("patVolume") / 100) }
        wake()
    }
    /// 锁定双击: the second click stays an ordinary press (pat, hold, drag). Returns whether the console opened.
    func openConsoleOnDoubleClick() -> Bool {
        guard !flag("lockDoubleClick") else { return false }
        pet_hold_cancel(core, now); showSettings(); return true
    }
    /// Same thresholds as Windows: 0.5 s, 30 points; hidden pet or open menu cancels.
    @discardableResult
    func updateHold(cursor: NSPoint? = nil) -> Bool {
        guard core != nil, holdPending else { return false }
        if hidden || menuOpen {
            pet_hold_cancel(core, now)
            holdPending = false
        } else {
            let cursor = cursor ?? NSEvent.mouseLocation
            holdPending = pet_hold_update(core, NSEvent.pressedMouseButtons & 1 != 0,
                                          Float(cursor.x), Float(cursor.y), 30, now) != 0
        }
        return holdPending
    }
    @objc func feedApple() { guard core != nil else { return }; pet_feed(core, now); wake() }
    func wake(interactive: Bool = false) {
        needsRedraw = true
        if interactive { awakeUntil = ProcessInfo.processInfo.systemUptime + 0.35 }
        guard !hidden else { return }
        // Wake even when the static pet has no timer. Repeated events leave the frame deadline intact.
        setTickRate(displayHz)
    }
    func applyWindow() {
        pet.ignoresMouseEvents = flag("clickThrough")
        pet.level = flag("alwaysOnTop") ? .floating : .normal
        let size = NSSize(width: 360 * number("scale"), height: 420 * number("scale"))
        pet.setContentSize(size)
        let pixels = RenderBudget.size(points: size)
        if pixels != renderSize && pet_resize(core, UInt32(pixels.width), UInt32(pixels.height)) {
            renderSize = pixels; needsRedraw = true; nextDraw = 0
        }
    }
    // An idle visible pet needs one movement notification to resume full refresh.
    // During animation the display link samples the current location once per frame.
    private func stopIdlePointerMonitor() {
        if let token = idleLocalMove { NSEvent.removeMonitor(token); idleLocalMove = nil }
        if let token = idleGlobalMove { NSEvent.removeMonitor(token); idleGlobalMove = nil }
    }
    func pointerMoved() {
        guard !hidden else { return }
        pointerAwakeUntil = ProcessInfo.processInfo.systemUptime + 0.18
        if timerRate != displayHz { wake() }
    }
    private func armIdlePointerMonitor() -> Bool {
        if smoke { return true } // Benchmark stimuli dispatch pointerMoved directly.
        if idleGlobalMove != nil && idleLocalMove != nil { return true }
        let mask: NSEvent.EventTypeMask = [.mouseMoved, .leftMouseDragged, .rightMouseDragged, .otherMouseDragged]
        idleGlobalMove = NSEvent.addGlobalMonitorForEvents(matching: mask) { [weak self] _ in self?.pointerMoved() }
        idleLocalMove = NSEvent.addLocalMonitorForEvents(matching: mask) { [weak self] event in self?.pointerMoved(); return event }
        if idleGlobalMove != nil && idleLocalMove != nil { return true }
        stopIdlePointerMonitor()
        return false
    }
    private func refreshDisplayRate() {
        let screen = pet.screen ?? NSScreen.main
        displayHz = Double(max(1, screen?.maximumFramesPerSecond ?? 60))
        if #unavailable(macOS 14), let id = screen?.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber,
           let mode = CGDisplayCopyDisplayMode(id.uint32Value), mode.refreshRate > 0 {
            displayHz = mode.refreshRate
        }
    }
    func windowDidChangeScreen(_ notification: Notification) {
        guard let window = notification.object as? NSWindow, window === pet else { return }
        refreshDisplayRate()
        if !startupPending { wake() }
    }
    @available(macOS 14.0, *)
    @objc private func displayDidRefresh(_ link: CADisplayLink) {
        displayTick = true
        defer { displayTick = false }
        autoreleasepool { tick() }
    }
    private func setTickRate(_ rate: Double) {
        guard timerRate != rate else { return }
        timer?.invalidate(); timer = nil; timerRate = rate
        if #available(macOS 14.0, *) {
            if let link = displayClock as? CADisplayLink { link.invalidate(); displayClock = nil }
            if rate == displayHz && !hidden {
                let link = view.displayLink(target: self, selector: #selector(displayDidRefresh(_:)))
                let hz = Float(displayHz)
                link.preferredFrameRateRange = CAFrameRateRange(minimum: hz, maximum: hz, preferred: hz)
                displayClock = link
                link.add(to: .main, forMode: .common)
                return
            }
        }
        guard rate > 0 else { return }
        let t = Timer(timeInterval: 1 / rate, repeats: true) { [weak self] _ in autoreleasepool { self?.tick() } }
        t.tolerance = 0.001; RunLoop.main.add(t, forMode: .common); timer = t
    }
    func changed() {
        if consoleClient { sendEditedControls(); return }
        previewSettings()
        applyWindow(); wake()
        saveStatus?.stringValue = "待保存…"
        saveTimer?.invalidate()
        saveTimer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: false) { [weak self] _ in self?.save() }
    }
    func previewSettings() {
        metal?.invalidateTextures()
        guard let data = try? JSONSerialization.data(withJSONObject: settings),
              let json = String(data: data, encoding: .utf8) else { return }
        if !json.withCString({ pet_preview(core, $0) }) { showCoreError() }
    }
    func save() {
        saveTimer?.invalidate(); saveTimer = nil
        guard core != nil, let data = try? JSONSerialization.data(withJSONObject: settings),
              let json = String(data: data, encoding: .utf8) else { return }
        if !json.withCString({ pet_apply(core, $0) }) { saveStatus?.stringValue = "保存失败"; showCoreError(); loadSettings(); applyWindow(); wake() }
        else { saveStatus?.stringValue = "已保存"; consoleChannel?.send(["status":"已保存"]) }
    }
    func savePosition() {
        settings["position"] = ["x": pet.frame.origin.x, "y": pet.frame.origin.y]
        save()
    }
    func tick() {
        guard core != nil else { return }
        let clock = ProcessInfo.processInfo.systemUptime
        let time = Float(clock - start)
        let elapsed = min(0.1, max(0, clock - lastTick))
        lastTick = clock
        if !smoke { input.recoverReleasedKeys() }
        if hidden {
            updateHold()
            stopIdlePointerMonitor()
            setTickRate(2)
            return
        }

        let tracksGaze = flag("trackMouse") && !flag("wine")
        let fadesOnHover = flag("clickThrough")
        let keyboardEnabled = flag("keyboardEnabled")
        let tracksShift = keyboardEnabled && (settings["posture"] as? String) == "standing"
        let r = pet.frame
        let cursor = benchmarkCursor ?? ((tracksGaze || fadesOnHover || holdPending) ? NSEvent.mouseLocation : .zero)
        let gaze = NSPoint(x: cursor.x - r.minX, y: r.maxY - cursor.y)
        let gazeChanged = tracksGaze && gaze != drawnGaze
        let shift = tracksShift && !cpuTest && CGEventSource.flagsState(.combinedSessionState).contains(.maskShift)
        if shift != lastShift { wake(interactive: true) }
        lastShift = shift
        if holdPending { updateHold(cursor: cursor) }

        let demand = pet_frame_demand(core, time)
        let fps = RenderBudget.fps(demand: demand, interacting: clock < awakeUntil,
                                   gazeChanged: gazeChanged, requested: displayHz)
        let rate = needsRedraw ? displayHz : fps
        if rate != drawRate {
            // Admit a faster animation immediately. Preserve a slower frame deadline
            // so a single mouse move cannot cause two back-to-back frames.
            if drawRate == 0 || rate > drawRate { nextDraw = 0 }
            drawRate = rate
        }
        if rate > 0 {
            let period = 1 / rate
            if displayTick || clock + min(0.001, period * 0.1) >= nextDraw {
                let dt = Float(min(0.15, max(0.001, clock - lastDraw)))
                if renderFrame(gaze: gaze, size: r.size, shift: shift, time: time, dt: dt) {
                    renderedFrames += 1
                    needsRedraw = false; drawnGaze = gaze
                    if captureWindow?.isVisible == true { captureView?.bitmap = view.bitmap }
                } else {
                    needsRedraw = true // The compositor can temporarily retain every buffer.
                }
                lastDraw = clock
                // Follow the timer's cadence without trying to replay missed frames.
                nextDraw += period
                if nextDraw <= clock { nextDraw = clock + period }
            }
        }

        var fading = false
        if fadesOnHover {
            let over = pet_hit(core, Float(gaze.x / r.width), Float(gaze.y / r.height))
            let target: CGFloat = over ? 0.25 : 1
            let alpha = pet.alphaValue
            let delta = target - alpha
            fading = abs(delta) > 0.002
            if fading { pet.alphaValue = alpha + delta * min(1, elapsed / 0.045) }
            else if alpha != target { pet.alphaValue = target }
        } else if pet.alphaValue != 1 {
            pet.alphaValue = 1
        }

        // Sample pointer-dependent changes on the same display clock as the animation.
        // A still pet with no pending behavior can let AppKit sleep until the next event.
        let needsPointer = tracksGaze || fadesOnHover || holdPending
        let idlePointer = needsPointer && !holdPending && !fading && rate == 0 && clock >= pointerAwakeUntil
        let canSleep = idlePointer && armIdlePointerMonitor()
        if !idlePointer { stopIdlePointerMonitor() }
        let pointerRate = needsPointer && !canSleep ? displayHz : 0
        let blinkRate = flag("daBlink") ? 30.0 : 0
        let inputRate = tracksShift || input.hasKeysToRecover || flag("hypQuickChat") ? 20.0 : 0
        let gestureRate = flag("occasionalStretch") || flag("occasionalTilt") || flag("occasionalSleepy") ? 5.0 : 0
        setTickRate(max(fps, pointerRate, blinkRate, inputRate, gestureRate, needsRedraw ? rate : 0))
    }
    private func installRenderer(_ renderer: MetalTestRenderer) {
        metal = renderer
        view.wantsLayer = true
        renderer.layer.frame = view.bounds
        renderer.layer.autoresizingMask = [.layerWidthSizable, .layerHeightSizable]
        view.layer?.addSublayer(renderer.layer)
    }
    private func renderFrame(gaze: NSPoint, size: NSSize, shift: Bool, time: Float, dt: Float) -> Bool {
        let x = Float(gaze.x), y = Float(gaze.y), w = Float(size.width), h = Float(size.height)
        let bytes: UnsafePointer<UInt8>?
        if let metal, captureWindow?.isVisible != true {
            var mesh = PetMetalMesh()
            let begin = benchmarkBreakdown ? clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) : 0
            let ready = pet_metal_mesh(core, x, y, w, h, shift, time, dt, &mesh)
            let projected = benchmarkBreakdown ? clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) : 0
            if benchmarkBreakdown { benchmarkProjectCPU += projected - begin }
            if ready {
                let submitted = metal.draw(mesh, size: renderSize)
                if benchmarkBreakdown { benchmarkSubmitCPU += clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) - projected }
                guard submitted else {
                    consecutiveGPUFailures += 1
                    if consecutiveGPUFailures >= 3 {
                        metal.layer.removeFromSuperlayer()
                        self.metal = nil
                        consecutiveGPUFailures = 0
                        if metal.usesSurfaces, let fallback = try? MetalTestRenderer(trackPresentations: cpuTest) {
                            installRenderer(fallback)
                        }
                    }
                    return false
                }
                consecutiveGPUFailures = 0
                if metal.layer.isHidden { metal.layer.isHidden = false }
                if view.bitmap != nil {
                    // A transparent GPU frame must not reveal a stale software frame.
                    view.bitmap = nil
                    view.needsDisplay = true
                    let cleanup = DispatchWorkItem { [weak self] in
                        guard let self, self.core != nil, self.view.bitmap == nil else { return }
                        self.view.releaseSoftwareBuffers()
                        pet_release_raster(self.core)
                        self.rasterCleanup = nil
                    }
                    rasterCleanup = cleanup
                    DispatchQueue.main.asyncAfter(deadline: .now() + 2, execute: cleanup)
                }
                metalFrames += 1
                return true
            }
            bytes = pet_pixels(core)
            metal.hideForSoftware()
        } else {
            metal?.hideForSoftware()
            bytes = pet_render(core, x, y, w, h, shift, time, dt)
        }
        rasterCleanup?.cancel(); rasterCleanup = nil
        guard let bytes, view.updatePixels(bytes, width: Int(renderSize.width), height: Int(renderSize.height)) else { return false }
        softwareFrames += 1
        return true
    }
    func configureInput() {
        if !smoke { input.configure(keyboard: flag("keyboardEnabled"), quickChat: flag("hypQuickChat")) }
        updatePermission()
    }
    @objc func grantInput() {
        NSApp.activate(ignoringOtherApps: true)
        _ = CGRequestListenEventAccess()
        configureInput()
        // Also when a stale grant from a previous build reports "allowed" but no keys arrive.
        if !paneSatisfied(.inputMonitoring) { showGuide(.inputMonitoring) }
    }
    @objc func grantAccessibility() {
        NSApp.activate(ignoringOtherApps: true)
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        _ = AXIsProcessTrustedWithOptions(options)
        configureInput()
        if !input.accessibilityAllowed { showGuide(.accessibility) }
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
        let inputOK = paneSatisfied(.inputMonitoring)
        consoleChannel?.send(["input":inputOK, "access":access])
        permissionSummary?.attributedStringValue = PermissionLook.summary(keyboardAuthorized: inputOK, chatAuthorized: access)
        inputRequestButton?.title = inputOK ? "输入监控已授权" : (listen ? "重新授权输入监控" : "申请输入监控")
        accessRequestButton?.title = access ? "辅助功能已授权" : "申请辅助功能"
        permissionLabel?.stringValue = "\(keyboard)。只用于角色动作，不保存或上传输入内容。"
        let problem = PermissionLook.problem(listen: listen, access: access, inputOK: inputOK)
        let level = PermissionLook.level(inputOK: inputOK, access: access)
        accessLabel?.stringValue = (problem.map { "✕ \($0)" } ?? "✓ 权限正常")
            + "\n输入监控：\(listen ? "已授权" : "待授权") · 辅助功能：\(access ? "已授权" : "待授权") · \(keyboard) · \(chat)"
        inputHealthLabel?.stringValue = input.diagnosticSummary
        // Whole window: green with both items, yellow with one, red with neither.
        // A layer behind the controls: sheets draw their own material over backgroundColor.
        permissionTint?.layer?.backgroundColor = PermissionLook.color(level).cgColor
        refreshGuide()
        // Opt-in diagnostics contain counts and permissions, never key text/codes.
        if CommandLine.arguments.contains("--input-diagnostics"),
           let data = try? JSONSerialization.data(withJSONObject: input.diagnostic, options: [.prettyPrinted, .sortedKeys]) {
            try? data.write(to: dataDir.appendingPathComponent("input-status.json"), options: .atomic)
        }
    }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if LaunchMode.ignoresReopen(arguments: launchArguments, secondsSinceLaunch: ProcessInfo.processInfo.systemUptime - start,
                                    delay: startupDelay) { return false }
        showSettings(); return true
    }
    func applicationWillTerminate(_ notification: Notification) {
        if consoleClient { consoleChannel?.drain(); return }
        if let process = consoleProcess, process.isRunning { process.terminate() }
        guard core != nil else { return }
        stopIdlePointerMonitor()
        if #available(macOS 14.0, *) { (displayClock as? CADisplayLink)?.invalidate() }
        closeCapture(); capePanel?.orderOut(nil); capePanel = nil
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
        if consoleClient { consoleChannel?.send(["action":"download", "name":input]); return }
        fetchSkinForName(named: input)
    }
    func fetchSkinForName(named input: String) {
        let compact = input.replacingOccurrences(of: "-", with: "")
        let isUUID = compact.count == 32 && compact.allSatisfy { $0.isHexDigit }
        let isName = input.count <= 16 && input.unicodeScalars.allSatisfy { CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_").contains($0) }
        guard isUUID || isName else { alert("请输入有效的 Java 版用户名或 UUID。"); return }
        let requestID = UUID(); downloadID = requestID
        updateDownloadStatus("正在连接官方皮肤服务…")
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
                updateDownloadStatus("已载入：\(name) · \(capeBytes == nil ? "该账号未装备披风" : "皮肤与披风已保存")")
            } catch { if requestID == downloadID { updateDownloadStatus("获取失败，保留原皮肤与披风：\(error.localizedDescription)") } }
        }
    }
    func updateDownloadStatus(_ text: String) {
        onlineStatus?.stringValue = text; consoleChannel?.send(["online":text])
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
        if forwardConsole("reset") { return }
        downloadID = UUID(); saveTimer?.invalidate()
        if !smoke && flag("autostart") {
            do { try LoginStartup.setEnabled(false, app: Bundle.main.bundleURL, data: dataDir, directory: loginDirectory) }
            catch { alert(error.localizedDescription); return }
        }
        if !pet_reset(core) { showCoreError(); return }
        loadSettings(); configureMusic(); applyWindow(); configureInput(); recenter(); wake(); panel = closePanel(); showSettings()
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
    // --watch: print once a second for 30 s, including Apple Music/Spotify broadcasts.
    let watcher = PlayerStateWatcher()
    let rounds = CommandLine.arguments.contains("--watch") ? 30 : 1
    if rounds > 1 { watcher.start() }
    for round in 0..<rounds {
        if round > 0 { RunLoop.main.run(until: Date().addingTimeInterval(1)) }
        let result = AudioOutputProbe.snapshot(reported: watcher.reported)
        let reported = watcher.reported.map { "\($0.key)=\($0.value ? "playing" : "paused")" }.sorted().joined(separator: ",")
        print("[\(round)] Music playing: \(result.playing.map(String.init) ?? "unavailable"); source: \(result.source); players: \(result.players.joined(separator: ",")); reported: \(reported)")
        for line in result.processes { print("    " + line) }
        fflush(stdout)
    }
    exit(0)
}
if CommandLine.arguments.contains("--self-test-input") { InputSelfTests.run(); exit(0) }
if CommandLine.arguments.contains(where: { ["--cpu-test", "--self-test-ui", "--smoke-test", "--memory-test", "--self-test-startup", "--self-test-restart"].contains($0) })
    && !CommandLine.arguments.contains("--data-dir") { fputs("UI tests require --data-dir\n", stderr); exit(2) }
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
