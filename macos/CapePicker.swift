import AppKit

struct CapeEntry: Decodable { let id: String; let name: String }

extension AppDelegate {
    @objc func showCapes() {
        if let capePanel { capePanel.makeKeyAndOrderFront(nil); return }
        guard let ptr = pet_cape_catalog(core), let data = String(cString: ptr).data(using: .utf8),
              let entries = try? JSONDecoder().decode([CapeEntry].self, from: data) else { return }
        capeEntries = entries; capePage = 0
        let w = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 648, height: 556),
                        styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        w.title = "自选披风 · Minecraft桌宠"; w.isReleasedWhenClosed = false; w.delegate = self
        capePanel = w; refreshCapes(); w.center(); w.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
    func refreshCapes() {
        guard let w = capePanel else { return }
        let root = NSView(frame: NSRect(x: 0, y: 0, width: 648, height: 556)); w.contentView = root
        let choices = [CapeEntry(id: "account", name: "跟随皮肤原有披风"), CapeEntry(id: "none", name: "无披风")] + capeEntries
        let selected = settings["capeChoice"] as? String ?? "account"
        for slot in 0..<12 {
            let index = capePage * 12 + slot
            guard choices.indices.contains(index) else { continue }
            let entry = choices[index]
            let b = NSButton(title: entry.name, target: self, action: #selector(selectCape(_:)))
            b.frame = NSRect(x: 16 + slot % 4 * 156, y: 82 + (2 - slot / 4) * 152, width: 148, height: 144)
            b.tag = index; b.setButtonType(.pushOnPushOff); b.bezelStyle = .regularSquare
            b.state = selected == entry.id ? .on : .off; b.font = .systemFont(ofSize: 11)
            b.imagePosition = .imageAbove; b.imageScaling = .scaleNone
            b.setAccessibilityLabel(entry.name + (selected == entry.id ? "（已选择）" : ""))
            if index >= 2 {
                var length = 0
                if let png = pet_cape_png(UInt32(index - 2), &length), length > 0,
                   let rep = NSBitmapImageRep(data: Data(bytes: png, count: length)),
                   let image = rep.cgImage?.cropping(to: CGRect(x: 1, y: 1, width: 10, height: 16)) {
                    // Retain only the twelve visible thumbnails, not every decoded texture.
                    let thumbnail = NSImage(size: NSSize(width: 50, height: 80), flipped: false) { rect in
                        guard let context = NSGraphicsContext.current?.cgContext else { return false }
                        context.interpolationQuality = .none; context.draw(image, in: rect); return true
                    }
                    b.image = thumbnail
                }
            }
            root.addSubview(b)
        }
        let previous = action(root, "上一页", #selector(previousCapePage), 12, 26, 110)
        previous.isEnabled = capePage > 0
        let next = action(root, "下一页", #selector(nextCapePage), 526, 26, 110)
        next.isEnabled = (capePage + 1) * 12 < choices.count
        let name = choices.first { $0.id == selected }?.name ?? "未选择"
        let status = label(root, "\(capePage + 1)/\((choices.count + 11) / 12) · \(name)", 126, 29, 396)
        status.alignment = .center
    }
    @objc func previousCapePage() { capePage = max(0, capePage - 1); refreshCapes() }
    @objc func nextCapePage() {
        if (capePage + 1) * 12 < capeEntries.count + 2 { capePage += 1 }; refreshCapes()
    }
    @objc func selectCape(_ sender: NSButton) {
        let choices = ["account", "none"] + capeEntries.map(\.id)
        guard choices.indices.contains(sender.tag) else { return }
        save() // Flush unsaved controls before the core persists the cape choice.
        guard choices[sender.tag].withCString({ pet_select_cape(core, $0) }) else { showCoreError(); return }
        loadSettings(); wake(); refreshCapes(); refreshConsoleControls()
    }
}
