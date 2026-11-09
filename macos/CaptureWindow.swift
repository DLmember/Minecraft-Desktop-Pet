import AppKit

/// Ordinary opaque window for OBS/window capture. Shares immutable CGImages with
/// PetView: no second pose, renderer, timer, or unbounded frame queue.
final class CaptureView: NSView {
    var bitmap: CGImage? { didSet { needsDisplay = true } }
    override var isOpaque: Bool { true }
    static func imageRect(image: NSSize, bounds: NSRect) -> NSRect {
        guard image.width > 0, image.height > 0 else { return .zero }
        let scale = min(bounds.width / image.width, bounds.height / image.height)
        let size = NSSize(width: image.width * scale, height: image.height * scale)
        return NSRect(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2, width: size.width, height: size.height)
    }
    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        ctx.setFillColor(NSColor(srgbRed: 0, green: 1, blue: 0, alpha: 1).cgColor)
        ctx.fill(bounds)
        guard let bitmap else { return }
        let rect = Self.imageRect(image: NSSize(width: bitmap.width, height: bitmap.height), bounds: bounds)
        ctx.saveGState(); ctx.interpolationQuality = .none
        // This view uses AppKit's bottom-left origin, unlike flipped PetView.
        ctx.draw(bitmap, in: rect); ctx.restoreGState()
    }
}

extension AppDelegate {
    @objc func showCapture() {
        if let w = captureWindow { w.makeKeyAndOrderFront(nil); captureParked = false; return }
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 450, height: 560),
                         styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        w.title = "Minecraft桌宠 · 直播采集（绿色背景）"
        w.isReleasedWhenClosed = false; w.delegate = self; w.isOpaque = true; w.hasShadow = false
        w.backgroundColor = NSColor(srgbRed: 0, green: 1, blue: 0, alpha: 1)
        w.contentMinSize = NSSize(width: 160, height: 180)
        let v = CaptureView(frame: w.contentView!.bounds); v.autoresizingMask = [.width, .height]
        v.bitmap = view.bitmap; w.contentView = v
        captureView = v; captureWindow = w; captureParked = false
        w.center(); w.makeKeyAndOrderFront(nil); wake()
    }
    @objc func toggleCapturePlacement() {
        if captureWindow == nil { showCapture() }
        guard let w = captureWindow else { return }
        captureParked.toggle()
        if captureParked { w.orderBack(nil) } else { w.makeKeyAndOrderFront(nil) }
    }
    func closeCapture() {
        captureView?.bitmap = nil
        let w = captureWindow; captureWindow = nil; captureView = nil; captureParked = false
        w?.orderOut(nil)
    }
}
