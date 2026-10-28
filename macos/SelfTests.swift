import AppKit
import ApplicationServices

enum InputSelfTests {
    static func run() {
        var count = 0
        func check(_ condition: @autoclosure () -> Bool, _ name: String) {
            guard condition() else { fputs("FAIL: \(name)\n", stderr); exit(1) }
            count += 1
        }
        func event(_ type: CGEventType, _ text: String, _ key: CGKeyCode = 20, _ flags: CGEventFlags = []) -> CGEvent {
            let event = CGEvent(keyboardEventSource: CGEventSource(stateID: .privateState), virtualKey: key, keyDown: type == .keyDown)!
            event.type = type; event.flags = flags
            let utf16 = Array(text.utf16); event.keyboardSetUnicodeString(stringLength: utf16.count, unicodeString: utf16)
            return event
        }
        let chat = QuickChat()
        check(!TapHealth.hasKeys(0x1000), "flags-only tap is not a working keyboard connection")
        check(!TapHealth.hasKeys(0x1400), "missing keyUp is not a working connection")
        check(TapHealth.hasKeys(0x1c00), "complete keyboard mask is accepted")
        let routed = event(.keyDown, "#", 20, .maskShift)
        routed.setIntegerValueField(.eventTargetUnixProcessID, value: 42)
        let copied = QuickChat.replacementEvents(original: routed)!
        check(copied.allSatisfy { $0.getIntegerValueField(.eventTargetUnixProcessID) == 42 }, "replacement preserves original routing")
        check(KeyboardText.translated(routed) == "#", "source event is not mutated")
        let input = InputController()
        var observed: [(UInt32, Bool)] = []
        input.onKey = { observed.append(($0, $1)) }
        // Exercise the production event-to-animation path, without installing a tap.
        input.observe(type: .keyDown, event: event(.keyDown, "a", 0))
        check(input.keyDownCount == 1, "production handler counts ordinary letters")
        check(observed.isEmpty, "disabled motion does not dispatch keys")
        input.observe(type: .keyUp, event: event(.keyUp, "a", 0))
        check(input.keyUpCount == 1, "production handler receives releases")
        let injected = event(.keyDown, "a", 0)
        injected.setIntegerValueField(.eventSourceUserData, value: QuickChat.tag)
        input.observe(type: .keyDown, event: injected)
        check(input.keyDownCount == 1, "injected prefix does not trigger extra animation")
        check(chat.decide(type: .keyDown, event: event(.keyDown, "#"), enabled: false) == .pass, "disabled preserves hash")
        check(chat.decide(type: .keyDown, event: event(.keyDown, "#", 20, .maskCommand), enabled: true) == .pass, "Command shortcut")
        check(chat.decide(type: .keyDown, event: event(.keyDown, "#", 20, .maskControl), enabled: true) == .pass, "Control shortcut")
        check(chat.decide(type: .keyDown, event: event(.keyDown, "＃"), enabled: true) == .pass, "full-width hash unchanged")
        check(chat.decide(type: .keyDown, event: event(.keyDown, "3"), enabled: true) == .pass, "plain 3 unchanged")
        check(chat.decide(type: .keyDown, event: event(.keyDown, "#", 20, .maskShift), enabled: true) == .replace, "US Shift+3")
        let repeatEvent = event(.keyDown, "#"); repeatEvent.setIntegerValueField(.keyboardEventAutorepeat, value: 1)
        check(chat.decide(type: .keyDown, event: repeatEvent, enabled: true) == .swallow, "repeat produces no second prefix")
        check(chat.decide(type: .keyUp, event: event(.keyUp, "#"), enabled: false) == .swallow, "matching release consumed even after disabling")
        check(chat.swallowed.isEmpty, "release clears held hash")
        check(chat.decide(type: .keyDown, event: repeatEvent, enabled: true) == .pass, "enabling mid-hold does not transform repeats")
        check(chat.decide(type: .keyDown, event: event(.keyDown, "#", 42), enabled: true) == .replace, "layout-specific hash key")
        chat.reset()
        check(chat.decide(type: .keyDown, event: event(.keyDown, "#", 20, .maskAlternate), enabled: true) == .replace, "Mac UK Option+3 hash")
        chat.reset()
        let own = event(.keyDown, "#"); own.setIntegerValueField(.eventSourceUserData, value: QuickChat.tag)
        check(chat.decide(type: .keyDown, event: own, enabled: true) == .pass, "injected input cannot recurse")
        let events = QuickChat.replacementEvents()!
        check(events.count == 16, "eight balanced key pairs")
        var text = ""
        for (index, e) in events.enumerated() {
            check(e.flags.isEmpty, "Shift/Caps do not alter replacement")
            check(e.getIntegerValueField(.eventSourceUserData) == QuickChat.tag, "all injection tagged")
            check(e.getIntegerValueField(.keyboardEventKeycode) != 36, "no Return injected")
            check(e.type == (index % 2 == 0 ? .keyDown : .keyUp), "balanced key order")
            if e.type == .keyDown { text += KeyboardText.translated(e) ?? "" }
        }
        check(text == "/p chat ", "exact replacement including trailing space")
        check(!KeyboardText.modifierDown(code: 56, flags: CGEventFlags(rawValue: 0x20000 | 0x4)), "left Shift release while right held")
        check(KeyboardText.modifierDown(code: 60, flags: CGEventFlags(rawValue: 0x20000 | 0x4)), "right Shift stays held")
        check(KeyboardText.modifierDown(code: 59, flags: CGEventFlags(rawValue: 0x40000 | 0x1)), "left Control event flags")
        print("Input self-tests: \(count) passed")
        let inputCount = count
        for delay in LoginStartup.delays {
            check(LoginStartup.delay(arguments: ["pet", "--autostart"], enabled: true, seconds: delay) == delay, "login delay")
            check(LoginStartup.delay(arguments: ["pet"], enabled: true, seconds: delay) == 0, "manual launch has no delay")
            check(LoginStartup.delay(arguments: ["pet", "--autostart"], enabled: false, seconds: delay) == 0, "disabled login has no delay")
        }
        check(LoginStartup.delay(arguments: ["--autostart"], enabled: true, seconds: 999) == 0, "invalid delay rejected")
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("pet-platform-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        do {
            let app = root.appendingPathComponent("Pet ' 中文 $() &.app")
            let executable = app.appendingPathComponent("Contents/MacOS/MinecraftPet")
            try FileManager.default.createDirectory(at: executable.deletingLastPathComponent(), withIntermediateDirectories: true)
            try Data().write(to: executable)
            let data = root.appendingPathComponent("Profile data")
            let agents = root.appendingPathComponent("LaunchAgents")
            let target = agents.appendingPathComponent(LoginStartup.label + ".plist")
            try LoginStartup.setEnabled(true, app: app, data: data, directory: agents)
            let bytes = try Data(contentsOf: target)
            let plist = try PropertyListSerialization.propertyList(from: bytes, format: nil) as! [String: Any]
            let args = plist["ProgramArguments"] as! [String]
            check(args == ["/usr/bin/open", "-g", "-a", app.path, "--args", "--autostart", "--data-dir", data.path], "structured arguments preserve spaces and shell characters")
            check(plist["RunAtLoad"] as? Bool == true && plist["KeepAlive"] == nil, "login only; never respawn on quit")
            try LoginStartup.setEnabled(true, app: app, data: data, directory: agents)
            let repeated = try Data(contentsOf: target)
            check(repeated == bytes, "registration is idempotent")
            do {
                try LoginStartup.setEnabled(false, app: app, data: root, directory: agents)
                check(false, "another profile cannot remove agent")
            } catch { check(FileManager.default.fileExists(atPath: target.path), "foreign agent retained") }
            try LoginStartup.setEnabled(false, app: app, data: data, directory: agents)
            check(!FileManager.default.fileExists(atPath: target.path), "disable removes only own plist")
            try LoginStartup.setEnabled(false, app: app, data: data, directory: agents)
            let moved = root.appendingPathComponent("Moved.app")
            try FileManager.default.moveItem(at: app, to: moved)
            try LoginStartup.setEnabled(true, app: moved, data: data, directory: agents)
            let updated = try PropertyListSerialization.propertyList(from: Data(contentsOf: target), format: nil) as! [String: Any]
            check((updated["ProgramArguments"] as! [String])[3] == moved.path, "moved application path updates")
            for value in RenderPriority.values {
                try RenderPriority.apply(value)
                var requested = QOS_CLASS_UNSPECIFIED
                let result = pthread_get_qos_class_np(pthread_self(), &requested, nil)
                check(result == 0 && requested == RenderPriority.qos(value), "requested QoS reads back")
            }
            try RenderPriority.apply("normal")
            check(RenderPriority.qos("invalid") == RenderPriority.qos("normal"), "unknown QoS defaults to normal")
            let valid = try OfficialTexture.address("http://textures.minecraft.net/texture/abcdef123456")
            check(valid == "https://textures.minecraft.net/texture/abcdef123456", "legacy official URL upgraded to HTTPS")
            for bad in ["https://evil.example/texture/abc", "https://textures.minecraft.net.evil.example/texture/abc",
                "file:///texture/abc", "https://user@textures.minecraft.net/texture/abc", "https://textures.minecraft.net:123/texture/abc",
                "https://textures.minecraft.net/texture/../abc", "https://textures.minecraft.net/texture/xyz",
                "https://textures.minecraft.net/texture/abc?redirect=1", "https://textures.minecraft.net/texture/abc#fragment"] {
                do { _ = try OfficialTexture.address(bad); check(false, "untrusted texture rejected") }
                catch { check(true, "untrusted texture rejected") }
            }
        } catch { fputs("FAIL: platform test: \(error)\n", stderr); exit(1) }
        print("Platform self-tests: \(count-inputCount) passed; total \(count)")
    }
}
