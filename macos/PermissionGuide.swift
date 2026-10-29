import AppKit
import ApplicationServices

/// Privacy & Security lists that accept an app dragged in from Finder.
enum PrivacyPane {
    case inputMonitoring, accessibility
    var title: String { self == .inputMonitoring ? "输入监控" : "辅助功能" }
    /// tccutil(1) service name; also the System Settings anchor after "Privacy_".
    var service: String { self == .inputMonitoring ? "ListenEvent" : "Accessibility" }
    var url: URL { URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_\(service)")! }
    static let appBundleID = "com.dlmember.minecraft-desktop-pet.macos"
    /// Always scoped to this app: a bare `tccutil reset <service>` clears every app's grant.
    func resetArguments(bundleID: String?) -> [String]? {
        guard let id = bundleID, id == PrivacyPane.appBundleID else { return nil }
        return ["reset", service, id]
    }
}

/// The app itself as a Finder-style drag: System Settings adds a dropped
/// bundle URL to the open list, the same as dragging it out of /Applications.
final class DraggableAppView: NSView, NSDraggingSource {
    let bundleURL: URL
    private var downEvent: NSEvent?
    init(frame: NSRect, bundleURL: URL) {
        self.bundleURL = bundleURL
        super.init(frame: frame)
        wantsLayer = true
        layer?.cornerRadius = 8; layer?.borderWidth = 1
        layer?.backgroundColor = NSColor.controlBackgroundColor.cgColor
        layer?.borderColor = NSColor.separatorColor.cgColor
        let icon = NSImageView(frame: NSRect(x: 10, y: (frame.height - 36) / 2, width: 36, height: 36))
        icon.image = NSWorkspace.shared.icon(forFile: bundleURL.path); addSubview(icon)
        let name = NSTextField(labelWithString: FileManager.default.displayName(atPath: bundleURL.path))
        name.font = .systemFont(ofSize: 14, weight: .semibold)
        name.frame = NSRect(x: 56, y: (frame.height - 20) / 2, width: frame.width - 66, height: 20); addSubview(name)
        setAccessibilityLabel("拖动 \(name.stringValue) 到系统设置列表")
    }
    required init?(coder: NSCoder) { fatalError() }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .openHand) }
    // Subviews must not swallow the drag.
    override func hitTest(_ point: NSPoint) -> NSView? { frame.contains(point) ? self : nil }
    override func mouseDown(with event: NSEvent) { downEvent = event }
    override func mouseDragged(with event: NSEvent) {
        guard let down = downEvent else { return }
        let dx = event.locationInWindow.x - down.locationInWindow.x, dy = event.locationInWindow.y - down.locationInWindow.y
        guard hypot(dx, dy) >= 3 else { return }
        downEvent = nil
        let item = NSDraggingItem(pasteboardWriter: bundleURL as NSURL)
        let image = bitmapImageRepForCachingDisplay(in: bounds).map { rep -> NSImage in
            cacheDisplay(in: bounds, to: rep); let i = NSImage(size: bounds.size); i.addRepresentation(rep); return i
        } ?? NSWorkspace.shared.icon(forFile: bundleURL.path)
        item.setDraggingFrame(bounds, contents: image)
        beginDraggingSession(with: [item], event: down, source: self).animatesToStartingPositionsOnCancelOrFail = true
    }
    override func mouseUp(with event: NSEvent) { downEvent = nil }
    func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        context == .outsideApplication ? [.copy, .generic, .link] : []
    }
}

/// Frame of the frontmost System Settings window in AppKit screen coordinates.
/// Window bounds are readable without Screen Recording; names are not needed.
enum SystemSettingsWindow {
    static func frame() -> NSRect? {
        let pids = Set(NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.systempreferences").map { $0.processIdentifier })
        guard !pids.isEmpty, let primary = NSScreen.screens.first,
              let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[CFString: Any]] else { return nil }
        var best: CGRect?
        for info in list {
            guard let pid = info[kCGWindowOwnerPID] as? pid_t, pids.contains(pid), info[kCGWindowLayer] as? Int == 0,
                  let bounds = info[kCGWindowBounds] as? NSDictionary,
                  let rect = CGRect(dictionaryRepresentation: bounds), rect.width >= 300, rect.height >= 200 else { continue }
            if best == nil || rect.width * rect.height > best!.width * best!.height { best = rect }
        }
        // CoreGraphics: top-left origin on the primary display.
        return best.map { NSRect(x: $0.minX, y: primary.frame.maxY - $0.maxY, width: $0.width, height: $0.height) }
    }
    /// Beside the Settings window: right if it fits, else left, else inside.
    static func placement(for size: NSSize, beside window: NSRect, in visible: NSRect) -> NSPoint {
        let y = min(max(window.maxY - size.height - 40, visible.minY), visible.maxY - size.height)
        if window.maxX + 12 + size.width <= visible.maxX { return NSPoint(x: window.maxX + 12, y: y) }
        if window.minX - 12 - size.width >= visible.minX { return NSPoint(x: window.minX - 12 - size.width, y: y) }
        return NSPoint(x: max(visible.minX, window.maxX - size.width - 16), y: max(visible.minY, window.minY + 16))
    }
}

extension PermissionLook {
    static func color(_ level: Level) -> NSColor {
        switch level {
        case .green: return NSColor(srgbRed: 0.13, green: 0.55, blue: 0.27, alpha: 1)
        case .yellow: return NSColor(srgbRed: 0.78, green: 0.58, blue: 0.05, alpha: 1)
        case .red: return NSColor(srgbRed: 0.76, green: 0.16, blue: 0.14, alpha: 1)
        }
    }
}

extension AppDelegate {
    func paneSatisfied(_ pane: PrivacyPane) -> Bool {
        pane == .inputMonitoring
            ? PermissionLook.inputSatisfied(listen: input.listeningAllowed, keyboardEnabled: flag("keyboardEnabled"), keyboardRunning: input.running)
            : input.accessibilityAllowed
    }
    /// Open the matching Privacy list and float a small card with the app to drag in.
    func showGuide(_ pane: PrivacyPane) {
        if !smoke { NSWorkspace.shared.open(pane.url) }
        closeGuide()
        let size = NSSize(width: 340, height: 262)
        let w = NSPanel(contentRect: NSRect(origin: .zero, size: size),
                        styleMask: [.titled, .closable, .utilityWindow, .nonactivatingPanel], backing: .buffered, defer: false)
        w.title = "拖放授权 · \(pane.title)"
        // The user works in System Settings meanwhile; keep the card visible.
        w.hidesOnDeactivate = false; w.isFloatingPanel = true; w.level = .floating
        w.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        w.isReleasedWhenClosed = false; w.delegate = self
        let root = w.contentView!
        let head = label(root, "把下面的图标拖进「\(pane.title)」列表", 16, 226, 308)
        head.font = .systemFont(ofSize: 14, weight: .semibold)
        root.addSubview(DraggableAppView(frame: NSRect(x: 16, y: 166, width: 308, height: 52), bundleURL: Bundle.main.bundleURL))
        let steps = label(root, "1. 按住上面的「\(appDisplayName)」，拖到旁边系统设置的列表里。\n2. 打开它右侧的开关。\n3. 列表里已有旧的「\(appDisplayName)」时，先点「清除旧记录」，再拖一次。", 16, 82, 308, 76)
        steps.lineBreakMode = .byWordWrapping; steps.font = .systemFont(ofSize: 12)
        guideStatus = label(root, "", 16, 46, 308, 32)
        guideStatus?.font = .systemFont(ofSize: 12, weight: .medium); guideStatus?.lineBreakMode = .byWordWrapping
        action(root, "清除旧记录", #selector(resetGuideRecords), 10, 14, 112)
        action(root, "重启应用", #selector(restartApplication), 124, 14, 100)
        action(root, "完成", #selector(closeGuide), 236, 14, 94)
        guidePanel = w; guidePane = pane
        placeGuide(); refreshGuide()
        w.orderFrontRegardless()
        let timer = Timer(timeInterval: 0.25, repeats: true) { [weak self] _ in self?.placeGuide(); self?.refreshGuide() }
        RunLoop.main.add(timer, forMode: .common); guideTimer = timer
    }
    var appDisplayName: String { FileManager.default.displayName(atPath: Bundle.main.bundlePath) }
    func placeGuide() {
        guard let w = guidePanel, let visible = (NSScreen.main ?? NSScreen.screens.first)?.visibleFrame else { return }
        guard let settings = SystemSettingsWindow.frame() else {
            // Until System Settings appears, wait near the top-right corner.
            if !w.isVisible { w.setFrameOrigin(NSPoint(x: visible.maxX - w.frame.width - 24, y: visible.maxY - w.frame.height - 24)) }
            return
        }
        let origin = SystemSettingsWindow.placement(for: w.frame.size, beside: settings, in: visible)
        if w.frame.origin != origin { w.setFrameOrigin(origin) }
    }
    func refreshGuide() {
        guard let pane = guidePane, let status = guideStatus else { return }
        if paneSatisfied(pane) {
            status.stringValue = "已授权 ✓"; status.textColor = .systemGreen
        } else if pane == .inputMonitoring && (input.listeningAllowed || input.accessibilityAllowed) {
            status.stringValue = "系统显示已授权，但按键仍未连接：点「清除旧记录」后重新拖入，或点「重启应用」。"; status.textColor = .systemOrange
        } else {
            status.stringValue = "等待授权…"; status.textColor = .secondaryLabelColor
        }
    }
    /// Remove this bundle's entries for one service; tccutil needs no admin rights for that.
    @objc func resetGuideRecords() {
        guard let pane = guidePane, let arguments = pane.resetArguments(bundleID: Bundle.main.bundleIdentifier) else { return }
        let tool = Process(); tool.executableURL = URL(fileURLWithPath: "/usr/bin/tccutil")
        tool.arguments = arguments
        tool.standardOutput = FileHandle.nullDevice; tool.standardError = FileHandle.nullDevice
        do { try tool.run(); tool.waitUntilExit() } catch { guideStatus?.stringValue = "无法清除旧记录：\(error.localizedDescription)"; return }
        guard tool.terminationStatus == 0 else { guideStatus?.stringValue = "清除失败，请在列表里手动删除旧条目。"; return }
        input.refresh(force: true); updatePermission()
        // Reopen so the list redraws without the removed entries.
        NSWorkspace.shared.open(pane.url)
        guideStatus?.stringValue = "已清除旧记录，现在把图标拖进列表。"; guideStatus?.textColor = .labelColor
    }
    @objc func closeGuide() {
        guideTimer?.invalidate(); guideTimer = nil
        let w = guidePanel; guidePanel = nil; guidePane = nil; guideStatus = nil
        w?.orderOut(nil)
    }
}
