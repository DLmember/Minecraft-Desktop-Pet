import AppKit
import ApplicationServices
import Carbon

/// Translate text with isolated dead-key state; never mutate the user's input source.
enum KeyboardText {
    static func translated(_ event: CGEvent) -> String? {
        var chars = [UniChar](repeating: 0, count: 8)
        var count = 0
        event.keyboardGetUnicodeString(maxStringLength: chars.count, actualStringLength: &count, unicodeString: &chars)
        if count > 0 && count <= chars.count { return String(utf16CodeUnits: chars, count: count) }
        guard let source = TISCopyCurrentKeyboardLayoutInputSource()?.takeRetainedValue(),
              let pointer = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else { return nil }
        let data = Unmanaged<CFData>.fromOpaque(pointer).takeUnretainedValue()
        guard let bytes = CFDataGetBytePtr(data) else { return nil }
        let layout = UnsafeRawPointer(bytes).assumingMemoryBound(to: UCKeyboardLayout.self)
        let flags = event.flags
        var modifiers: UInt32 = 0
        if flags.contains(.maskShift) { modifiers |= UInt32(shiftKey) }
        if flags.contains(.maskAlternate) { modifiers |= UInt32(optionKey) }
        if flags.contains(.maskAlphaShift) { modifiers |= UInt32(alphaLock) }
        var dead: UInt32 = 0
        let status = UCKeyTranslate(layout, UInt16(event.getIntegerValueField(.keyboardEventKeycode)),
            UInt16(kUCKeyActionDown), modifiers >> 8, UInt32(LMGetKbdType()),
            OptionBits(kUCKeyTranslateNoDeadKeysMask), &dead, chars.count, &count, &chars)
        return status == noErr && count > 0 ? String(utf16CodeUnits: chars, count: count) : nil
    }
    static func modifierDown(code: UInt16, flags: CGEventFlags) -> Bool {
        // CGEventSource polling in a tap can still describe the PREVIOUS event.
        // Use per-device modifier bits carried by this flagsChanged event.
        let masks: [UInt16: UInt64] = [59:0x1,56:0x2,60:0x4,55:0x8,54:0x10,58:0x20,61:0x40,62:0x2000]
        if code == 57 { return flags.contains(.maskAlphaShift) }
        if code == 63 { return flags.contains(.maskSecondaryFn) }
        return masks[code].map { flags.rawValue & $0 != 0 } ?? false
    }
}

/// Same invariants as native/src/hyp_chat.rs: match text, consume repeats/up,
/// tag our injected events, preserve shortcuts, never use clipboard or Return.
final class QuickChat {
    static let tag: Int64 = 0x4D435048
    static let text = "/p chat "
    private(set) var swallowed = Set<UInt16>()
    enum Decision: Equatable { case pass, swallow, replace }
    func decide(type: CGEventType, event: CGEvent, enabled: Bool) -> Decision {
        if event.getIntegerValueField(.eventSourceUserData) == Self.tag { return .pass }
        let code = UInt16(clamping: event.getIntegerValueField(.keyboardEventKeycode))
        if swallowed.contains(code) {
            if type == .keyUp { swallowed.remove(code) }
            return .swallow
        }
        guard enabled, type == .keyDown,
              event.getIntegerValueField(.keyboardEventAutorepeat) == 0,
              !event.flags.contains(.maskCommand), !event.flags.contains(.maskControl),
              KeyboardText.translated(event) == "#" else { return .pass }
        swallowed.insert(code)
        return .replace
    }
    func release(_ code: UInt16) { swallowed.remove(code) }
    func reset() { swallowed.removeAll() }
    static func replacementEvents(original: CGEvent? = nil) -> [CGEvent]? {
        let source = CGEventSource(stateID: .privateState)
        // Real corresponding keycodes help apps that inspect both key and text.
        let codes: [CGKeyCode] = [44,35,49,8,4,0,17,49]
        var events: [CGEvent] = []
        for (index, unit) in text.utf16.enumerated() {
            for down in [true, false] {
                guard let e = original?.copy() ?? CGEvent(keyboardEventSource: source, virtualKey: codes[index], keyDown: down) else { return nil }
                e.type = down ? .keyDown : .keyUp
                e.setIntegerValueField(.keyboardEventKeycode, value: Int64(codes[index]))
                e.setIntegerValueField(.keyboardEventAutorepeat, value: 0)
                e.flags = []
                e.setIntegerValueField(.eventSourceUserData, value: tag)
                e.keyboardSetUnicodeString(stringLength: 1, unicodeString: [unit])
                events.append(e)
            }
        }
        return events
    }
}

/// macOS may remove keyDown/keyUp but leave an enabled flagsChanged-only tap.
enum TapHealth {
    static let keys: CGEventMask = (1 << CGEventType.keyDown.rawValue) | (1 << CGEventType.keyUp.rawValue)
    static let mask: CGEventMask = keys | (1 << CGEventType.flagsChanged.rawValue)
    static func hasKeys(_ mask: CGEventMask) -> Bool { mask & keys == keys }
    static func installed() -> [CGEventTapInformation]? {
        var count: UInt32 = 0
        guard CGGetEventTapList(0, nil, &count) == .success else { return nil }
        var entries = [CGEventTapInformation](repeating: CGEventTapInformation(), count: Int(count) + 16)
        let capacity = UInt32(entries.count)
        guard CGGetEventTapList(capacity, &entries, &count) == .success else { return nil }
        return Array(entries.prefix(Int(min(count, capacity))))
    }
}

/// Main-run-loop owner. No disk, network, UI or renderer work inside the tap.
final class InputController {
    var onKey: ((UInt32, Bool) -> Void)?
    var onReset: (() -> Void)?
    var onStateChange: (() -> Void)?
    private(set) var keyboardEnabled = false
    private(set) var quickChatEnabled = false
    private(set) var tap: CFMachPort?
    private var source: CFRunLoopSource?
    private(set) var activeFilter = false
    private var lastMode = -1
    private var lastAttempt: TimeInterval = 0
    private var tapID: UInt32?
    private var lastPermissions = ""
    private var localMonitor: Any?
    private(set) var effectiveMask: CGEventMask = 0
    private(set) var failure = ""
    private(set) var keyDownCount: UInt64 = 0
    private(set) var keyUpCount: UInt64 = 0
    private(set) var replacementCount: UInt64 = 0
    private(set) var timeoutCount: UInt64 = 0
    let quickChat = QuickChat()
    struct Press { var observed = false; var upSince: TimeInterval? }
    private var held: [UInt16: Press] = [:]
    private var swallowedPhysical: [UInt16: Press] = [:]
    var listeningAllowed: Bool { CGPreflightListenEventAccess() }
    var accessibilityAllowed: Bool { AXIsProcessTrusted() }
    var secureInput: Bool { IsSecureEventInputEnabled() }
    var running: Bool { tap.map { CGEvent.tapIsEnabled(tap: $0) } == true && TapHealth.hasKeys(effectiveMask) }
    var quickChatReady: Bool { quickChatEnabled && running && activeFilter && !secureInput }
    var diagnostic: [String: Any] {
        ["pid": getpid(), "version": Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "",
         "appPath": Bundle.main.bundlePath, "inputMonitoring": listeningAllowed,
         "accessibility": accessibilityAllowed, "secureInput": secureInput,
         "keyboardEnabled": keyboardEnabled, "quickChatEnabled": quickChatEnabled,
         "running": running, "activeFilter": activeFilter, "effectiveMask": String(effectiveMask, radix:16),
         "failure": failure, "keyDownCount": keyDownCount, "keyUpCount": keyUpCount,
         "replacementCount": replacementCount, "timeoutCount": timeoutCount]
    }
    var diagnosticSummary: String {
        if secureInput { return "系统安全输入已开启，暂时无法获取全局按键。" }
        return "收到按下 \(keyDownCount) / 松开 \(keyUpCount) · 井号替换 \(replacementCount) 次\n\(failure)"
    }
    func configure(keyboard: Bool, quickChat: Bool) {
        if keyboardEnabled && !keyboard { held.removeAll(); onReset?() }
        let changed = keyboardEnabled != keyboard || quickChatEnabled != quickChat
        keyboardEnabled = keyboard; quickChatEnabled = quickChat
        if localMonitor == nil {
            localMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { [weak self] event in
                // Local interaction needs no global permission. Avoid double dispatch.
                if let self, !self.running, let cg = event.cgEvent { self.observe(type: cg.type, event: cg) }
                return event
            }
        }
        refresh(force: changed)
    }
    func refresh(force: Bool = false) {
        let clock = ProcessInfo.processInfo.systemUptime
        if let tapID, let entries = TapHealth.installed() {
            effectiveMask = entries.first(where: { $0.eventTapID == tapID })?.eventsOfInterest ?? 0
        }
        let permissions = "\(listeningAllowed)-\(accessibilityAllowed)"
        let permissionChanged = permissions != lastPermissions
        // A preflight check can be stale after replacing a locally signed build.
        // Attempt the actual connection, then validate its effective event mask.
        let desired = quickChatEnabled ? 2 : (keyboardEnabled ? 1 : 0)
        // Finish consuming the matching release if the option was toggled mid-press.
        if !quickChat.swallowed.isEmpty && activeFilter && running { return }
        if !force && !permissionChanged && desired == lastMode && (desired == 0 || (running && (desired == 1 || activeFilter))) { return }
        if !force && !permissionChanged && clock-lastAttempt < 3 { return }
        lastAttempt = clock; lastPermissions = permissions
        // Retrying chat must not tear down a working passive keyboard listener.
        let keepListener = running && !activeFilter && desired == 2 && !force && !permissionChanged
        if !keepListener { stopTap() }
        lastMode = desired
        guard desired != 0 else { onStateChange?(); return }
        let callback: CGEventTapCallBack = { proxy, type, event, context in
            guard let context else { return Unmanaged.passUnretained(event) }
            let owner = Unmanaged<InputController>.fromOpaque(context).takeUnretainedValue()
            return owner.receive(proxy: proxy, type: type, event: event)
        }
        let modes = desired == 2 && keyboardEnabled && !keepListener ? [2, 1] : [desired]
        failure = ""
        for mode in modes {
            let before = Set(TapHealth.installed()?.map { $0.eventTapID } ?? [])
            // Keep both modes at the same routing stage, including events sent
            // directly to an application (accessibility keyboards, remote input).
            let location: CGEventTapLocation = .cgAnnotatedSessionEventTap
            let options: CGEventTapOptions = mode == 2 ? .defaultTap : .listenOnly
            guard let created = CGEvent.tapCreate(tap: location, place: .headInsertEventTap,
                options: options, eventsOfInterest: TapHealth.mask, callback: callback,
                userInfo: Unmanaged.passUnretained(self).toOpaque()) else {
                failure = "系统拒绝键盘监听，请重新授权并重启应用。"; continue
            }
            let info = TapHealth.installed()?.first {
                !before.contains($0.eventTapID) && $0.tappingProcess == getpid() && $0.tapPoint == location && $0.options == options
            }
            let candidateMask = info?.eventsOfInterest ?? 0
            guard TapHealth.hasKeys(candidateMask) else {
                CGEvent.tapEnable(tap: created, enable: false); CFMachPortInvalidate(created)
                if !keepListener { effectiveMask = candidateMask }
                failure = "系统未开放普通按键（mask=0x\(String(candidateMask, radix:16))），请重新授权并重启。"
                continue
            }
            if keepListener { stopTap() }
            effectiveMask = candidateMask
            tap = created; tapID = info?.eventTapID; activeFilter = mode == 2
            source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, created, 0)
            CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
            CGEvent.tapEnable(tap: created, enable: true)
            if mode == desired { failure = "" }
            break
        }
        onStateChange?()
    }
    func observe(type: CGEventType, event: CGEvent) {
        guard type == .keyDown || type == .keyUp || type == .flagsChanged,
              event.getIntegerValueField(.eventSourceUserData) != QuickChat.tag else { return }
        if type == .keyDown { keyDownCount += 1 }
        if type == .keyUp { keyUpCount += 1 }
        let code = UInt16(clamping: event.getIntegerValueField(.keyboardEventKeycode))
        let down = type == .flagsChanged ? KeyboardText.modifierDown(code: code, flags: event.flags) : type == .keyDown
        if keyboardEnabled, let vk = keyMap[code] {
            if down { if held[code] == nil { held[code] = Press() } }
            else { held.removeValue(forKey: code) }
            onKey?(vk, down)
        }
    }
    func receive(proxy: CGEventTapProxy, type: CGEventType, event: CGEvent) -> Unmanaged<CGEvent>? {
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            timeoutCount += 1; resetState()
            if let tap { CGEvent.tapEnable(tap: tap, enable: true) }
            return Unmanaged.passUnretained(event)
        }
        guard event.getIntegerValueField(.eventSourceUserData) != QuickChat.tag else { return Unmanaged.passUnretained(event) }
        observe(type: type, event: event)
        guard activeFilter, type == .keyDown || type == .keyUp else { return Unmanaged.passUnretained(event) }
        let code = UInt16(clamping: event.getIntegerValueField(.keyboardEventKeycode))
        // Replacements are synchronous copies in this very event stream, so
        // querying NSWorkspace's possibly stale foreground PID is unnecessary.
        let decision = quickChat.decide(type: type, event: event, enabled: quickChatEnabled)
        switch decision {
        case .pass: return Unmanaged.passUnretained(event)
        case .swallow:
            if type == .keyUp { swallowedPhysical.removeValue(forKey: code) }
            return nil
        case .replace:
            guard let replacements = QuickChat.replacementEvents(original: event) else { quickChat.release(code); return Unmanaged.passUnretained(event) }
            swallowedPhysical[code] = Press(); replacementCount += 1
            // Insert synchronously into THIS event's stream: no queued focus race,
            // no later keystroke can overtake the prefix, and # never reaches the app.
            for replacement in replacements {
                replacement.tapPostEvent(proxy)
            }
            return nil
        }
    }
    func recoverReleasedKeys() {
        let clock = ProcessInfo.processInfo.systemUptime
        for code in Array(swallowedPhysical.keys) {
            guard var press = swallowedPhysical[code] else { continue }
            if CGEventSource.keyState(.hidSystemState, key: code) {
                press.observed = true; press.upSince = nil
            } else if press.observed {
                if press.upSince == nil { press.upSince = clock }
                if clock - (press.upSince ?? clock) >= 0.06 {
                    quickChat.release(code); swallowedPhysical.removeValue(forKey: code); continue
                }
            }
            swallowedPhysical[code] = press
        }
        for code in Array(held.keys) {
            guard var press = held[code] else { continue }
            if CGEventSource.keyState(.combinedSessionState, key: code) {
                press.observed = true; press.upSince = nil
            } else if press.observed {
                if press.upSince == nil { press.upSince = clock }
                if clock - (press.upSince ?? clock) >= 0.06 {
                    held.removeValue(forKey: code)
                    if let vk = keyMap[code] { onKey?(vk, false) }
                    continue
                }
            }
            held[code] = press
        }
    }
    func resetState() { held.removeAll(); swallowedPhysical.removeAll(); quickChat.reset(); onReset?() }
    private func stopTap() {
        if let tap { CGEvent.tapEnable(tap: tap, enable: false); CFMachPortInvalidate(tap) }
        if let source { CFRunLoopRemoveSource(CFRunLoopGetMain(), source, .commonModes) }
        tap = nil; source = nil; tapID = nil; effectiveMask = 0; activeFilter = false; resetState()
    }
    func stop() {
        stopTap(); lastMode = -1
        if let localMonitor { NSEvent.removeMonitor(localMonitor) }; localMonitor = nil
    }
}
