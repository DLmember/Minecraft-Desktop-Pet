import AppKit
import Darwin

extension AppDelegate {
    /// Relaunch the real signed bundle three times with its permission sheet
    /// open. Repeated button actions must never start parallel instances.
    func runRestartRegression() {
        let trace = dataDir.appendingPathComponent("restart-pids.json")
        var pids = (try? Data(contentsOf: trace)).flatMap { try? JSONDecoder().decode([Int32].self, from: $0) } ?? []
        for pid in pids {
            precondition(kill(pid, 0) == -1 && errno == ESRCH, "Previous instance must exit before the replacement starts")
        }
        pids.append(ProcessInfo.processInfo.processIdentifier)
        try! JSONEncoder().encode(pids).write(to: trace, options: .atomic)
        if pids.count == 4 {
            try! Data("Three restarts passed; previous processes exited; no overlapping instances.\n".utf8)
                .write(to: dataDir.appendingPathComponent("restart-passed.txt"), options: .atomic)
            NSApp.terminate(nil)
            return
        }
        showSettings(); showPermissions()
        let button = permissionPanel!.contentView!.subviews.compactMap { $0 as? NSButton }
            .first { $0.action == #selector(restartApplication) }!
        button.performClick(nil)
        button.performClick(nil)
        precondition(restartPending && permissionPanel == nil)
    }

    func runUIRegression() {
        let saved = settings
        precondition(StableVersion.parse("v2.5") == StableVersion.parse("2.5.0"))
        precondition(StableVersion.newer("v2.10", than: "2.9.1"))
        for tag in ["v2.5-beta","v2.5-blockbench.1","latest","v2.4.12","2.5.0"] {
            precondition(!StableVersion.newer(tag, than: "2.5.0"))
        }
        settings["clickThrough"] = false; settings["trackMouse"] = false; settings["hypQuickChat"] = true
        changed(); save(); loadSettings()
        precondition(settings["startupPriority"] == nil)
        precondition(controlTabs?.numberOfTabViewItems == 3)
        func checkCompactConsole() {
            let root = panel!.contentView!
            precondition(root.frame.size == ConsoleSize.compact && root.bounds.size == ConsoleSize.design)
            for button in pageButtons {
                let point = button.convert(NSPoint(x: button.bounds.midX, y: button.bounds.midY), to: nil)
                precondition(root.hitTest(point) === button, "Scaled navigation retains mouse hit testing")
            }
        }
        // A CGImage must own immutable pixels even when the pool resizes or advances.
        let originalSettings = settings
        for scale in [0.65, 1.0, 1.8] {
            settings["scale"] = scale; changed(); tick()
            let expected = RenderBudget.size(points: pet.frame.size)
            precondition(renderSize == expected && view.bitmap?.width == Int(expected.width)
                         && view.bitmap?.height == Int(expected.height), "Window/core/image geometry stays synchronized")
        }
        settings = originalSettings; changed(); tick()
        let wasHidden = hidden
        hidden = true; tick(); precondition(timerRate == 2, "Hidden pet uses low-frequency checks")
        hidden = false; wake(); precondition(timerRate == max(30, number("fps")), "Unhide wakes without waiting for the idle timer")
        hidden = wasHidden; wake()
        let probe = PetView(frame: NSRect(x: 0, y: 0, width: 16, height: 16))
        let red = [UInt8](repeating: 127, count: 16 * 16 * 4)
        red.withUnsafeBufferPointer { probe.updatePixels($0.baseAddress!, width: 16, height: 16) }
        let old = probe.bitmap!
        let sameSize = [UInt8](repeating: 19, count: 16 * 16 * 4)
        sameSize.withUnsafeBufferPointer { probe.updatePixels($0.baseAddress!, width: 16, height: 16) }
        precondition((old.dataProvider!.data! as Data).first == 127, "Next frame cannot overwrite a retained image")
        let blue = [UInt8](repeating: 43, count: 8 * 8 * 4)
        blue.withUnsafeBufferPointer { probe.updatePixels($0.baseAddress!, width: 8, height: 8) }
        precondition(probe.bitmap!.width == 8 && old.width == 16)
        let retainedBytes = old.dataProvider!.data! as Data
        precondition(retainedBytes.first == 127 && retainedBytes.count == red.count, "Resizing retains old frame memory safely")
        checkCompactConsole()
        func controls(_ view: NSView) -> [NSControl] {
            (view as? NSControl).map { [$0] } ?? view.subviews.flatMap { controls($0) }
        }
        func all() -> [NSControl] { controlTabs!.selectedTabViewItem!.view.map(controls) ?? [] }
        for key in ["autostart","typingLetters","daDance","musicNotes","teaseEnabled","idleAnimation","chibi",
                    "occasionalStretch","occasionalTilt","occasionalSleepy"] {
            guard let b = all().compactMap({$0 as? NSButton}).first(where:{bindings[ObjectIdentifier($0)]?.key == key}) else { preconditionFailure("Missing \(key)") }
            let expected = !flag(key)
            b.state = expected ? .on : .off; controlChanged(b); save(); loadSettings()
            precondition(flag(key) == expected, "Setting persists: \(key)")
        }
        func button(_ key: String) -> NSButton { all().compactMap { $0 as? NSButton }.first { bindings[ObjectIdentifier($0)]?.key == key }! }
        settings["daBlink"] = true; settings["chibi"] = false; changed(); save(); populatePage(selectedConsolePage)
        let chibi = button("chibi"); chibi.state = .on; controlChanged(chibi); save(); loadSettings()
        precondition(flag("chibi") && flag("daBlink"), "Turning on Q mode keeps DA blink (2.5.1)")
        precondition(button("daBlink").isEnabled && button("daBlink").state == .on, "DA blink stays available in Q mode")
        // 2.5.3: PAT喵！ row on 常规; keyboard style and colour moved to 自定义姿态.
        precondition(keyboardColorField == nil, "Keyboard colour left the 常规 page")
        let pat = button("patEnabled"), patOn = !flag("patEnabled")
        pat.state = patOn ? .on : .off; controlChanged(pat); save(); loadSettings()
        precondition(flag("patEnabled") == patOn, "PAT喵！ persists")
        let volume = all().compactMap { $0 as? NSSlider }.first { bindings[ObjectIdentifier($0)]?.key == "patVolume" }!
        volume.doubleValue = 72.4; controlChanged(volume); save(); loadSettings()
        precondition(settings["patVolume"] as? Int == 72, "Sound volume is stored as a whole percent")
        precondition(bindings[ObjectIdentifier(volume)]?.label?.stringValue == "72%")
        precondition(patSound.loaded, "Both PAT meows are bundled")
        let patRow = volume.frame.minY, otherRow = button("chibi").frame.minY, squashRow = all().compactMap { $0 as? NSSlider }
            .first { bindings[ObjectIdentifier($0)]?.key == "chibiBodyScale" }!.frame.minY
        precondition(otherRow < patRow && patRow < squashRow, "PAT row sits between Q 版身体大小 and 其他")
        let lock = lockDoubleClickBox!
        lock.state = .on; toggleLockDoubleClick(lock); save(); loadSettings()
        precondition(flag("lockDoubleClick") && !openConsoleOnDoubleClick(), "Locked double-click keeps the console closed")
        lock.state = .off; toggleLockDoubleClick(lock); save(); loadSettings()
        precondition(!flag("lockDoubleClick"))
        for (keyboard, chat) in [(false, false), (true, false), (false, true), (true, true)] {
            let text = PermissionLook.summaryText(keyboardAuthorized: keyboard, chatAuthorized: chat)
            precondition(text.keyboard == (keyboard ? "键盘互动已授权" : "键盘互动未授权") && text.chat == (chat ? "快速聊天已授权" : "快速聊天未授权"))
            let line = PermissionLook.summary(keyboardAuthorized: keyboard, chatAuthorized: chat)
            let first = line.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? NSColor
            let last = line.attribute(.foregroundColor, at: line.length - 1, effectiveRange: nil) as? NSColor
            precondition(first == (keyboard ? NSColor.systemGreen : .systemRed) && last == (chat ? NSColor.systemGreen : .systemRed),
                         "Status line: green when authorized, red when not")
        }
        let shown = permissionSummary!.attributedStringValue.string
        precondition(shown.hasPrefix("键盘互动") && shown.contains("快速聊天") && !shown.contains("连接"), "Console shows authorization, not connection")
        precondition(pageButtons[2].title == "其他设置")
        let wine = button("wine"); wine.state = .on; controlChanged(wine); save(); loadSettings()
        precondition(flag("wine") && pet_frame_demand(core, now) == 2, "Wine is persisted and continuously animated")
        wine.state = .off; controlChanged(wine); save()
        showCapes()
        precondition(capeEntries.count > 50 && capePanel!.isVisible)
        if let v = capePanel?.contentView, let rep = v.bitmapImageRepForCachingDisplay(in: v.bounds) {
            v.cacheDisplay(in: v.bounds, to: rep)
            try? rep.representation(using: .png, properties: [:])?.write(to: dataDir.appendingPathComponent("smoke-capes.png"))
        }
        let choose = capePanel!.contentView!.subviews.compactMap { $0 as? NSButton }.first { $0.tag == 2 && $0.action == #selector(selectCape(_:)) }!
        choose.performClick(nil); loadSettings()
        precondition(settings["capeChoice"] as? String == capeEntries[0].id)
        nextCapePage(); precondition(capePage == 1)
        previousCapePage(); precondition(capePage == 0)
        let noCape = capePanel!.contentView!.subviews.compactMap { $0 as? NSButton }.first { $0.tag == 1 && $0.action == #selector(selectCape(_:)) }!
        noCape.performClick(nil); loadSettings(); precondition(settings["capeChoice"] as? String == "none")
        capePanel!.performClose(nil); precondition(capePanel == nil && capeEntries.isEmpty)
        showCapture(); let capturePanel = captureWindow!
        precondition(capturePanel.isVisible && capturePanel.isOpaque && captureView!.bitmap === view.bitmap)
        let greenProbe = CaptureView(frame: NSRect(x: 0, y: 0, width: 20, height: 20))
        let imageRep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 20, pixelsHigh: 20,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: imageRep)
        greenProbe.draw(greenProbe.bounds)
        NSGraphicsContext.restoreGraphicsState()
        let green = imageRep.colorAt(x: 0, y: 0)!.usingColorSpace(.sRGB)!
        precondition(green.greenComponent > 0.99 && green.redComponent < 0.01 && green.blueComponent < 0.01 && green.alphaComponent > 0.99,
                     "Capture matte is opaque chroma green")
        precondition(CaptureView.imageRect(image: NSSize(width: 100,height:200), bounds: NSRect(x:0,y:0,width:300,height:300)) == NSRect(x:75,y:0,width:150,height:300))
        toggleCapturePlacement(); precondition(captureParked)
        toggleCapturePlacement(); precondition(!captureParked)
        settings["wine"] = true; changed(); tick()
        precondition(captureView!.bitmap === view.bitmap, "Capture reuses the same frame without a second rasterizer")
        settings["wine"] = false; changed()
        if let v = captureView, let rep = v.bitmapImageRepForCachingDisplay(in: v.bounds) {
            v.cacheDisplay(in: v.bounds, to: rep)
            try? rep.representation(using: .png, properties: [:])?.write(to: dataDir.appendingPathComponent("smoke-capture.png"))
        }
        capturePanel.performClose(nil); precondition(captureWindow == nil && captureView == nil)
        pageButtons[2].performClick(nil)
        for choice in ["none", "rabbit", "cat", "heart", "axolotl", "lollipop"] {
            let radio = all().compactMap { $0 as? NSButton }.first { bindings[ObjectIdentifier($0)]?.key == "decoration" && bindings[ObjectIdentifier($0)]?.choices.first as? String == choice }!
            radio.performClick(nil); save(); loadSettings(); tick()
            precondition(settings["decoration"] as? String == choice)
        }
        for style in ["simple", "detailed"] {
            let radio = all().compactMap { $0 as? NSButton }.first {
                bindings[ObjectIdentifier($0)]?.key == "keyboardStyle" && bindings[ObjectIdentifier($0)]?.choices.first as? String == style }!
            radio.performClick(nil); save(); loadSettings()
            precondition(settings["keyboardStyle"] as? String == style, "Keyboard style persists: \(style)")
        }
        let colorField = keyboardColorField!
        colorField.stringValue = " ffaacc "; applyKeyboardColor(); save(); loadSettings()
        precondition(settings["keyboardColor"] as? String == "#FFAACC" && colorField.stringValue == "#FFAACC", "Keyboard color normalizes and persists")
        colorField.stringValue = "#GG0000"; applyKeyboardColor(); loadSettings()
        precondition(settings["keyboardColor"] as? String == "#FFAACC" && colorField.stringValue == "#FFAACC", "Invalid color is rejected")
        for bad in ["#FFF", "##FFFFFF", "", "#00000000"] { precondition(Self.normalizedKeyboardColor(bad) == nil, bad) }
        pageButtons[0].performClick(nil)
        let menu = NSMenu(); buildMenu(menu)
        let tops = menu.items.map(\.title)
        for prefix in ["打开控制台", "触发动作", "姿态：", "外观", "大小：", "鼠标挥动：", "键盘互动：", "保持置顶", "完全鼠标穿透", "视线跟随鼠标", "桌面与启动", "退出"] {
            precondition(tops.contains { $0.hasPrefix(prefix) }, "Menu has \(prefix)")
        }
        let gestures = menu.items.first { $0.title == "触发动作" }!.submenu!.items.map(\.title)
        precondition(gestures == ["打招呼", "开心蹦蹦", "害羞", "鼓掌", "喂食苹果"], "Gesture submenu order")
        let sizeMenu = menu.items.first { $0.title.hasPrefix("大小：") }!.submenu!
        func click(_ item: NSMenuItem) { precondition(NSApp.sendAction(item.action!, to: item.target, from: item), "Menu action: \(item.title)") }
        click(sizeMenu.items.first { $0.title == "125%" }!); save(); loadSettings()
        precondition(abs(number("scale") - 1.25) < 0.001 && pet.frame.width == 450, "Menu size choice applies")
        click(sizeMenu.items.first { $0.title == "100%" }!); save()
        let topToggle = menu.items.first { $0.title == "保持置顶" }!, wasTop = flag("alwaysOnTop")
        click(topToggle); precondition(flag("alwaysOnTop") == !wasTop, "Menu toggle flips setting")
        click(topToggle); precondition(flag("alwaysOnTop") == wasTop, "Reused menu item flips back")
        let waveItem = menu.items.first { $0.title == "触发动作" }!.submenu!.items[0]
        click(waveItem); precondition(pet_needs_frames(core, now), "Wave gesture wakes the frame timer")
        precondition(!pet_gesture(core, 99, now), "Unknown gesture is rejected")
        let reverse = all().compactMap { $0 as? NSButton }.first {
            let binding = bindings[ObjectIdentifier($0)]
            return binding?.key == "mainHand" && binding?.choices.first as? String == "reverse"
        }!
        reverse.performClick(nil); save()
        precondition(reverse.state == .on)
        precondition(reverse.superview!.subviews.compactMap { $0 as? NSButton }.filter { $0.state == .on }.count == 1)
        let arm = all().compactMap { $0 as? NSButton }.first { bindings[ObjectIdentifier($0)]?.key == "armModel" && $0.state == .on }
        precondition(arm != nil, "Selecting one radio row preserves other groups")
        let delay = all().compactMap { $0 as? NSPopUpButton }.first { bindings[ObjectIdentifier($0)]?.key == "autostartDelay" }!
        delay.selectItem(at: delay.numberOfItems - 1); controlChanged(delay); save()
        pageButtons[2].performClick(nil)
        let fps = all().compactMap { $0 as? NSSlider }.first { bindings[ObjectIdentifier($0)]?.key == "fps" }!
        fps.doubleValue = 6; controlChanged(fps); save(); loadSettings()
        precondition(number("fps") == 240, "FPS slider uses supported frame-rate values")
        pageButtons[0].performClick(nil)
        let footerControls = controls(panel!.contentView!)
        let github = footerControls.compactMap { $0 as? NSButton }.first { $0.title == "GitHub" }!
        precondition(github.image != nil, "Bundled GitHub icon is present")
        precondition(!footerControls.compactMap { $0 as? NSTextField }.contains { $0.stringValue.contains("制作") }, "Console has no credit line")
        precondition(!footerControls.compactMap { $0 as? NSTextField }.contains { $0.stringValue.contains("Apple Music") })
        loadSettings(); precondition(settings["mainHand"] as? String == "reverse")
        precondition(number("autostartDelay") == 120)
        let before = mousePresses
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp, .otherMouseDown, .otherMouseUp] {
            let event = NSEvent.mouseEvent(with:type, location:NSPoint(x:100,y:100), modifierFlags:[],
                timestamp:ProcessInfo.processInfo.systemUptime, windowNumber:pet.windowNumber,
                context:nil,eventNumber:1,clickCount:1,pressure:0.5)!
            NSApp.sendEvent(event)
        }
        precondition(mousePresses == before+2, "No duplicate mouse dispatch")
        for index in 0..<3 {
            pageButtons[index].performClick(nil)
            precondition(selectedConsolePage == index && pageButtons[index].state == .on)
            precondition(controlTabs!.tabViewItems.filter {!($0.view?.subviews.isEmpty ?? true)}.count == 1)
            if index == 2 {
                precondition(all().filter {$0 is NSSlider}.count == 19, "One pose at a time")
                let selector = all().compactMap { $0 as? NSButton }.first {
                    let binding = bindings[ObjectIdentifier($0)]
                    return binding?.key == "customBase" && binding?.choices.first as? String == "sitting"
                }!
                selector.performClick(nil)
                let slider = all().compactMap {$0 as? NSSlider}.first!
                slider.doubleValue = 27; controlChanged(slider); save(); loadSettings()
                precondition(settings["posture"] as? String == "custom")
                precondition(value("customSittingPose.head.0") == 27)
                precondition(all().compactMap { $0 as? NSButton }.contains {
                    bindings[ObjectIdentifier($0)]?.key == "posture" && $0.state == .on
                }, "Pose editing updates the visible custom-pose selection")
            }
            if let v = panel?.contentView, let rep = v.bitmapImageRepForCachingDisplay(in:v.bounds) {
                v.cacheDisplay(in:v.bounds,to:rep)
                try? rep.representation(using:.png,properties:[:])?.write(to:dataDir.appendingPathComponent("smoke-tab-\(index).png"))
            }
        }
        showPermissions()
        precondition(inputRequestButton?.window === permissionPanel)
        precondition(accessRequestButton?.window === permissionPanel)
        if let field = diagnosticsField, let window = permissionPanel {
            window.makeFirstResponder(field)
            for event in QuickChat.replacementEvents()! {
                if let e = NSEvent(cgEvent:event) { NSApp.sendEvent(e) }
            }
            precondition(field.stringValue == QuickChat.text, "Exact quick-chat text insertion")
        }
        let inputOK = paneSatisfied(.inputMonitoring), access = input.accessibilityAllowed
        let problem = PermissionLook.problem(listen: input.listeningAllowed, access: access, inputOK: inputOK)
        let level = PermissionLook.level(inputOK: inputOK, access: access)
        let tint = NSColor(cgColor: permissionTint!.layer!.backgroundColor!)!.usingColorSpace(.sRGB)!
        let expected = PermissionLook.color(level).usingColorSpace(.sRGB)!
        precondition(permissionTint!.frame == permissionPanel!.contentView!.bounds, "Tint covers the whole window")
        precondition(abs(tint.redComponent - expected.redComponent) < 0.01 && abs(tint.greenComponent - expected.greenComponent) < 0.01,
                     "Permission window is green with both, yellow with one, red with neither")
        precondition(accessLabel!.stringValue.hasPrefix(problem == nil ? "✓" : "✕"))
        func capture(_ window: NSWindow, _ name: String) {
            // Cache the frame view so the tinted window background is included.
            guard let v = window.contentView?.superview, let rep = v.bitmapImageRepForCachingDisplay(in: v.bounds) else { return }
            v.cacheDisplay(in: v.bounds, to: rep)
            try? rep.representation(using: .png, properties: [:])?.write(to: dataDir.appendingPathComponent(name))
        }
        capture(permissionPanel!, "smoke-permissions.png")
        closePermissions()
        for pane in [PrivacyPane.inputMonitoring, .accessibility] {
            showGuide(pane)
            let guide = guidePanel!
            precondition(guide.isVisible && !guide.hidesOnDeactivate && guide.level == .floating, "Guide stays beside System Settings")
            let source = guide.contentView!.subviews.compactMap { $0 as? DraggableAppView }.first!
            precondition(source.bundleURL == Bundle.main.bundleURL, "Drag carries this app bundle")
            precondition(source.hitTest(NSPoint(x: source.frame.midX, y: source.frame.midY)) === source)
            capture(guide, "smoke-guide-\(pane.service).png")
            guide.performClose(nil)
            precondition(guidePanel == nil && guideTimer == nil && !guide.isVisible)
        }
        let beside = SystemSettingsWindow.placement(for: NSSize(width: 340, height: 262),
            beside: NSRect(x: 100, y: 200, width: 700, height: 500), in: NSRect(x: 0, y: 0, width: 1440, height: 900))
        precondition(beside == NSPoint(x: 812, y: 398), "Guide docks right of System Settings")
        let left = SystemSettingsWindow.placement(for: NSSize(width: 340, height: 262),
            beside: NSRect(x: 700, y: 200, width: 700, height: 500), in: NSRect(x: 0, y: 0, width: 1440, height: 900))
        precondition(left.x == 348, "Guide docks left when the right edge is full")
        pageButtons[0].performClick(nil)
        let console = panel
        for (key, toggle) in [("clickThrough", { self.toggleClickThrough() }), ("alwaysOnTop", { self.toggleTop() })] {
            let expected = !flag(key)
            toggle()
            precondition(panel === console && permissionPanel == nil, "Menu-bar toggles never reopen the console")
            let box = all().compactMap { $0 as? NSButton }.first { bindings[ObjectIdentifier($0)]?.key == key }!
            precondition(flag(key) == expected && box.state == (expected ? .on : .off), "Open console mirrors menu-bar toggle: \(key)")
        }
        settings = saved; changed(); save()
        for _ in 0..<5 { panel?.close(); precondition(panel == nil && controlTabs == nil && bindings.isEmpty && pageButtons.isEmpty && consoleWindow?.contentView?.subviews.isEmpty == true); showSettings(); checkCompactConsole() }
        controlTabs?.selectTabViewItem(at:0)
        print("UI regression passed: Windows-aligned native layout, GitHub icon without credit, radio groups, FPS slider, three lazy native pages, v2.5 settings, isolated poses, input permissions, quick chat, click dispatch, window release")
    }

    /// Explicit isolated test mode; never installed as a background service.
    func startMemoryExercise() {
        guard CommandLine.arguments.contains("--data-dir") else { fatalError("Tests require isolated data directory") }
        var samples: [[String:Any]] = []
        var phase = "idle"
        func at(_ delay: Double, _ block: @escaping () -> Void) {
            DispatchQueue.main.asyncAfter(deadline:.now()+delay) { autoreleasepool { block() } }
        }
        let sampler = Timer(timeInterval:1,repeats:true) { [weak self] _ in
            guard let self else { return }
            var info = task_vm_info_data_t()
            var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
            let status = withUnsafeMutablePointer(to:&info) { ptr in
                ptr.withMemoryRebound(to:integer_t.self,capacity:Int(count)) {
                    task_info(mach_task_self_,task_flavor_t(TASK_VM_INFO),$0,&count)
                }
            }
            if status == KERN_SUCCESS {
                samples.append(["seconds":self.now,"phase":phase,"physicalFootprint":info.phys_footprint,
                    "residentSize":info.resident_size,"frames":self.renderedFrames])
            }
        }
        RunLoop.main.add(sampler,forMode:.common)
        at(8) {
            phase = "animated"
            self.settings["idleAnimation"] = true; self.settings["typingLetters"] = true
            self.settings["musicNotes"] = true; self.settings["posture"] = "sitting"
            self.prepareGlyphs(); self.changed(); pet_music(self.core,true); self.feedApple()
            for n in 0..<20 { at(Double(n)*0.3) { pet_key(self.core,65,true,self.now); pet_key(self.core,65,false,self.now); self.wake() } }
        }
        at(16) { phase = "console"; self.showSettings() }
        for n in 0..<12 {
            at(24+Double(n)*0.6) {
                phase = "console-cycles"
                self.controlTabs?.selectTabViewItem(at:n%3)
                self.panel?.close(); self.showSettings()
            }
        }
        at(33) { phase = "closed"; self.panel?.close() }
        at(42) {
            phase = "hd-skin"
            if let i = CommandLine.arguments.firstIndex(of:"--test-skin"), CommandLine.arguments.count > i+1 {
                precondition(CommandLine.arguments[i+1].withCString {pet_import(self.core,$0)})
                self.loadSettings(); self.wake()
            }
        }
        at(54) { phase = "hidden"; self.hidden = true; self.pet.orderOut(nil); self.wake() }
        at(62) {
            phase = "restored"; self.hidden = false; self.pet.orderFrontRegardless()
            self.settings["idleAnimation"] = false; self.settings["musicNotes"] = false
            pet_music(self.core,false); self.changed()
        }
        at(73) {
            sampler.invalidate()
            let output:[String:Any] = ["samples":samples,"targetBytes":30_000_000,
                "peakSampledFootprint":samples.compactMap{$0["physicalFootprint"] as? UInt64}.max() ?? 0,
                "productionInputTapEnabled":false]
            if let data = try? JSONSerialization.data(withJSONObject:output,options:[.prettyPrinted,.sortedKeys]) {
                try? data.write(to:self.dataDir.appendingPathComponent("memory-report.json"))
            }
            print("Memory exercise completed"); NSApp.terminate(nil)
        }
    }
}

extension AppDelegate {
    /// Isolated benchmark: no event tap, player control, login registration or audio.
    /// Configuration lives in --data-dir; measure CPU time rather than instantaneous ps.
    func startCPUExercise() {
        precondition(smoke && CommandLine.arguments.contains("--data-dir"))
        func cpu() -> Double {
            var usage = rusage(); getrusage(RUSAGE_SELF, &usage)
            return Double(usage.ru_utime.tv_sec + usage.ru_stime.tv_sec)
                + Double(usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) / 1_000_000
        }
        let scenario = CommandLine.arguments.firstIndex(of: "--cpu-scenario").flatMap {
            CommandLine.arguments.indices.contains($0 + 1) ? CommandLine.arguments[$0 + 1] : nil
        } ?? "steady"
        var stimulus: Timer?
        if scenario == "interaction" {
            var n = 0
            stimulus = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: true) { [weak self] _ in
                guard let self else { return }
                switch n % 3 {
                case 0: pet_key(self.core, 65, true, self.now); pet_key(self.core, 65, false, self.now)
                case 1: _ = pet_gesture(self.core, 0, self.now)
                default: _ = pet_mouse(self.core, 0, true, 0, 0, self.now); pet_hold_cancel(self.core, self.now)
                }
                n += 1; self.wake()
            }
        }
        if scenario == "gaze" {
            settings["trackMouse"] = true; changed()
            stimulus = Timer.scheduledTimer(withTimeInterval: 1.0 / 60, repeats: true) { [weak self] _ in
                guard let self else { return }
                let t = Double(self.now)
                self.benchmarkCursor = NSPoint(x: self.pet.frame.midX + sin(t * 2) * 160,
                                              y: self.pet.frame.midY + cos(t * 2) * 120)
            }
        }
        if scenario == "hidden" { hidden = true; pet.orderOut(nil) }
        if scenario == "console" { showSettings() }
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
            let began = ProcessInfo.processInfo.systemUptime, used = cpu(), frames = self.renderedFrames
            DispatchQueue.main.asyncAfter(deadline: .now() + 8) {
                stimulus?.invalidate()
                let elapsed = ProcessInfo.processInfo.systemUptime - began
                var info = task_vm_info_data_t()
                var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / 4)
                _ = withUnsafeMutablePointer(to: &info) { ptr in
                    ptr.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                        task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
                    }
                }
                let report: [String: Any] = ["cpuPercent": (cpu() - used) / elapsed * 100,
                    "wallSeconds": elapsed, "frames": self.renderedFrames - frames,
                    "renderFPS": Double(self.renderedFrames - frames) / elapsed,
                    "width": self.view.bitmap?.width ?? 0, "height": self.view.bitmap?.height ?? 0,
                    "physicalFootprint": info.phys_footprint, "scenario": scenario]
                let bytes = try! JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
                try! bytes.write(to: self.dataDir.appendingPathComponent("cpu-report.json"))
                self.writeSmokeReport()
                print(String(data: bytes, encoding: .utf8)!)
                NSApp.terminate(nil)
            }
        }
    }
}
