import AppKit

/// Private, inherited pipes. Only the pet owns preferences, input, audio and rendering.
/// Exiting the settings process also releases AppKit's process-lifetime control caches.
final class ConsoleChannel {
    private let input: FileHandle
    private let output: FileHandle
    private let writer = DispatchQueue(label: "pet.settings.write")
    init(input: FileHandle, output: FileHandle) {
        self.input = input
        self.output = output
        _ = fcntl(output.fileDescriptor, F_SETNOSIGPIPE, 1)
    }
    func read(_ receive: @escaping ([String: Any]?) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async { [input] in
            var pending = Data()
            while true {
                let chunk = input.availableData
                guard !chunk.isEmpty else { break }
                pending.append(chunk)
                while let end = pending.firstIndex(of: 10) {
                    let message = try? JSONSerialization.jsonObject(with: pending[..<end]) as? [String: Any]
                    pending.removeSubrange(...end)
                    if let message { DispatchQueue.main.async { receive(message) } }
                }
                if pending.count > 1_048_576 { break }
            }
            DispatchQueue.main.async { receive(nil) }
        }
    }
    func send(_ message: [String: Any]) {
        guard var data = try? JSONSerialization.data(withJSONObject: message) else { return }
        data.append(10)
        writer.async { [output, data] in try? output.write(contentsOf: data) }
    }
    func drain() { writer.sync {} }
}

extension AppDelegate {
    func openConsoleSession() {
        if let process = consoleProcess, process.isRunning { consoleChannel?.send(["front": true]); return }
        guard let executable = Bundle.main.executableURL else { return }
        let process = Process(), inbound = Pipe(), outbound = Pipe()
        process.executableURL = executable
        process.arguments = ["--settings-window"]
        if smoke && CommandLine.arguments.contains("--console-roundtrip") {
            process.arguments?.append("--console-roundtrip")
        }
        process.standardInput = inbound; process.standardOutput = outbound
        let channel = ConsoleChannel(input: outbound.fileHandleForReading, output: inbound.fileHandleForWriting)
        do { try process.run() } catch { alert(error.localizedDescription); return }
        consoleWantsPermissionDetails = false
        consoleProcess = process; consoleChannel = channel
        channel.read { [weak self, weak process] message in
            guard let self, let process, self.consoleProcess === process else { return }
            guard let message else { self.consoleProcess = nil; self.consoleChannel = nil; self.consoleWantsPermissionDetails = false; return }
            self.receiveConsoleEdit(message)
        }
        channel.send(["open": settings, "page": selectedConsolePage, "input": paneSatisfied(.inputMonitoring), "access":input.accessibilityAllowed])
    }
    func startConsoleClient() {
        startupPending = false
        let channel = ConsoleChannel(input: .standardInput, output: .standardOutput)
        consoleChannel = channel
        channel.read { [weak self] message in
            guard let self else { return }
            guard let message else { NSApp.terminate(nil); return }
            if let prefs = message["open"] as? [String: Any] {
                self.settings = prefs; self.consoleSent = prefs
                self.selectedConsolePage = message["page"] as? Int ?? 0; self.showSettings()
            } else if let prefs = message["refresh"] as? [String: Any] {
                self.settings = prefs; self.consoleSent = prefs; self.refreshConsoleControls()
            }
            if let input = message["input"] as? Bool, let access = message["access"] as? Bool {
                if let listen = message["listen"] as? Bool, let details = message["details"] as? String, let health = message["health"] as? String {
                    self.displayPermissionDetails(listen: listen, inputOK: input, access: access, details: details, health: health)
                }
                self.permissionSummary?.attributedStringValue = PermissionLook.summary(keyboardAuthorized: input, chatAuthorized: access)
            }
            if let text = message["status"] as? String { self.saveStatus?.stringValue = text }
            if let text = message["online"] as? String { self.onlineStatus?.stringValue = text }
            if message["front"] != nil { self.panel?.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true) }
            if CommandLine.arguments.contains("--console-roundtrip"), let target = message["testToggle"] as? Bool,
               let button = self.lockDoubleClickBox {
                button.state = target ? .on : .off
                self.toggleLockDoubleClick(button)
                self.controlTabs?.selectTabViewItem(at: 1)
            }
            if CommandLine.arguments.contains("--console-roundtrip"), let open = message["testPermissions"] as? Bool {
                if open {
                    let button = self.panel?.contentView?.subviews.compactMap { $0 as? NSButton }.first { $0.action == #selector(self.showPermissions) }
                    precondition(button != nil)
                    button?.performClick(nil)
                    precondition(self.permissionPanel != nil && self.panel?.attachedSheet === self.permissionPanel)
                } else {
                    precondition(self.accessLabel?.stringValue.isEmpty == false && self.inputHealthLabel?.stringValue.isEmpty == false)
                    self.closePermissions()
                    precondition(self.permissionPanel == nil)
                }
            }
            if message["close"] != nil { self.panel?.close() }
        }
    }
    func sendEditedControls() {
        let old = consoleSent as NSDictionary
        let edited = settings.filter { !(($0.value as? NSObject)?.isEqual(old[$0.key]) ?? false) }
        guard !edited.isEmpty else { return }
        consoleSent = settings; saveStatus?.stringValue = "待保存…"
        consoleChannel?.send(["edit": edited])
    }
    private func receiveConsoleEdit(_ message: [String: Any]) {
        if let page = message["page"] as? Int, (0...2).contains(page) { selectedConsolePage = page }
        if let edited = message["edit"] as? [String: Any] {
            let previous = settings
            for (key,value) in edited where settings[key] != nil { settings[key] = value }
            if edited["autostart"] != nil && !smoke {
                do { try syncLoginStartup() } catch { settings = previous; refreshConsoleControls(); alert(error.localizedDescription); return }
            }
            changed()
            if edited["musicNotes"] != nil { configureMusic() }
            if edited["keyboardEnabled"] != nil || edited["hypQuickChat"] != nil {
                configureInput()
                if !smoke && edited["keyboardEnabled"] != nil && flag("keyboardEnabled") && !input.running { grantInput() }
                if !smoke && edited["hypQuickChat"] != nil && flag("hypQuickChat") && !input.accessibilityAllowed { grantAccessibility() }
            }
        }
        switch message["action"] as? String {
        case "save": save()
        case "import": importSkin()
        case "restore": restoreBuiltinSkin()
        case "download": if let name = message["name"] as? String { fetchSkinForName(named: name) }
        case "capes": showCapes()
        case "permissionDetails": consoleWantsPermissionDetails = true; updatePermission()
        case "endPermissionDetails": consoleWantsPermissionDetails = false
        case "grantInput": grantInput()
        case "grantAccessibility": grantAccessibility()
        case "refreshPermissions": refreshPermissions()
        case "restart": restartApplication()
        case "reset": resetSettings()
        case "capture": showCapture()
        case "placement": toggleCapturePlacement()
        default: break
        }
    }
    func forwardConsole(_ action: String) -> Bool {
        guard consoleClient else { return false }
        consoleChannel?.send(["action": action]); return true
    }
}
