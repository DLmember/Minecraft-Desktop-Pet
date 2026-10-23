import AppKit
import CoreAudio

/// Read HAL activity flags only. No audio tap, samples, microphone, Apple Events,
/// private framework or per-player allowlist is involved.
enum AudioOutputProbe {
    struct Snapshot {
        let playing: Bool?
        let source: String
    }
    static func address(_ selector: AudioObjectPropertySelector,
                        _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(mSelector: selector, mScope: scope, mElement: kAudioObjectPropertyElementMain)
    }
    static func word(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector) -> UInt32? {
        var property = address(selector), value: UInt32 = 0
        var size = UInt32(MemoryLayout<UInt32>.size)
        guard AudioObjectGetPropertyData(object, &property, 0, nil, &size, &value) == noErr,
              size == MemoryLayout<UInt32>.size else { return nil }
        return value
    }
    static func objects(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector,
                        scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> [AudioObjectID]? {
        var property = address(selector), size: UInt32 = 0
        guard AudioObjectHasProperty(object, &property),
              AudioObjectGetPropertyDataSize(object, &property, 0, nil, &size) == noErr,
              size <= 64 * 1024, size % 4 == 0 else { return nil }
        if size == 0 { return [] }
        var ids = [AudioObjectID](repeating: 0, count: Int(size) / 4)
        let status = ids.withUnsafeMutableBytes {
            AudioObjectGetPropertyData(object, &property, 0, nil, &size, $0.baseAddress!)
        }
        guard status == noErr, Int(size) <= ids.count * 4 else { return nil }
        return Array(ids.prefix(Int(size) / 4))
    }
    static func activity(_ values: [UInt32?]) -> Bool? {
        if values.contains(where: { $0 == 1 }) { return true }
        return values.contains(where: { $0 == nil }) ? nil : false
    }
    static func snapshot() -> Snapshot {
        let system = AudioObjectID(kAudioObjectSystemObject)
        // These selectors appeared with process objects in macOS 14.2. The
        // runtime property check also tolerates an unavailable/restarting HAL.
        if #available(macOS 14.2, *),
           let processes = objects(system, kAudioHardwarePropertyProcessObjectList) {
            let states = processes.compactMap { process -> UInt32? in
                // A process may exit between enumeration and reading its state.
                guard word(process, kAudioProcessPropertyPID) != UInt32(getpid()) else { return nil }
                return word(process, kAudioProcessPropertyIsRunningOutput)
            }
            return Snapshot(playing: activity(states.map(Optional.some)), source: "process-output")
        }
        // Compatibility path for macOS 12/13: enumerate all output-capable
        // devices, including headphones and virtual devices, without recording.
        // On duplex devices, an input-only client can also keep the device active.
        guard let devices = objects(system, kAudioHardwarePropertyDevices) else {
            return Snapshot(playing: nil, source: "unavailable")
        }
        var states: [UInt32?] = []
        for device in devices {
            guard let streams = objects(device, kAudioDevicePropertyStreams, scope: kAudioObjectPropertyScopeOutput),
                  !streams.isEmpty else { continue }
            states.append(word(device, kAudioDevicePropertyDeviceIsRunningSomewhere))
        }
        return Snapshot(playing: activity(states), source: "output-device")
    }
}

final class MusicMonitor {
    var onChange: ((Bool, String) -> Void)?
    private var timer: Timer?
    private var enabled = false
    private var busy = false
    private var generation = 0
    private var lastPlaying: Bool?
    private var lastMessage = ""
    private let queue = DispatchQueue(label: "pet.audio-output-state")
    func setEnabled(_ enabled: Bool) {
        guard enabled != self.enabled else { return }
        generation += 1; self.enabled = enabled
        lastPlaying = nil; lastMessage = ""
        timer?.invalidate(); timer = nil
        guard enabled else { onChange?(false, "音乐音符已关闭。"); return }
        poll()
        let t = Timer(timeInterval: 1, repeats: true) { [weak self] _ in self?.poll() }
        t.tolerance = 0.2
        RunLoop.main.add(t, forMode: .common); timer = t
    }
    func stop() { generation += 1; enabled = false; timer?.invalidate(); timer = nil }
    private func poll() {
        guard enabled, !busy else { return }
        busy = true; let token = generation
        queue.async { [weak self] in
            let state = AudioOutputProbe.snapshot()
            let message: String
            switch state.playing {
            case true?: message = "检测到音频输出活动。"
            case false?: message = "等待音频播放。"
            default: message = "暂时无法读取音频输出状态。"
            }
            DispatchQueue.main.async {
                guard let self else { return }; self.busy = false
                guard self.enabled, self.generation == token else { return }
                let playing = state.playing == true
                guard self.lastPlaying != playing || self.lastMessage != message else { return }
                self.lastPlaying = playing; self.lastMessage = message
                self.onChange?(playing, message)
            }
        }
    }
}

enum StableVersion {
    static func parse(_ tag: String) -> [UInt32]? {
        let t = tag.hasPrefix("v") || tag.hasPrefix("V") ? String(tag.dropFirst()) : tag
        let p = t.split(separator: ".", omittingEmptySubsequences: false)
        guard (2...3).contains(p.count) else { return nil }
        var result: [UInt32] = []
        for part in p {
            guard !part.isEmpty, part.utf8.allSatisfy({ (48...57).contains($0) }), let v = UInt32(part) else { return nil }
            result.append(v)
        }
        if result.count == 2 { result.append(0) }
        return result
    }
    static func newer(_ remote: String, than current: String) -> Bool {
        guard let r = parse(remote), let c = parse(current) else { return false }
        return c.lexicographicallyPrecedes(r)
    }
}

extension AppDelegate {
    func configureMusic() {
        guard !smoke else { return }
        music.setEnabled(flag("musicNotes"))
    }
    func refreshMusicLabel() {
        musicToggle?.toolTip = musicStatus + " 音符跟随系统音频输出活动；网页、游戏或保持音频流的静音程序也可能触发。"
    }
    func prepareGlyphs() {
        guard !glyphsReady else { return }
        var masks = [UInt8](repeating: 0, count: 95 * 576)
        autoreleasepool {
            let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 24, pixelsHigh: 24, bitsPerSample: 8,
                samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96, bitsPerPixel: 32)!
            let context = NSGraphicsContext(bitmapImageRep: rep)!
            NSGraphicsContext.saveGraphicsState(); NSGraphicsContext.current = context
            let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 20, weight: .bold), .foregroundColor: NSColor.white]
            for i in 0..<95 {
                memset(rep.bitmapData!, 0, 24 * 24 * 4)
                let text = String(UnicodeScalar(i + 32)!) as NSString
                let size = text.size(withAttributes: attributes)
                text.draw(at: NSPoint(x: (24-size.width)/2, y: (24-size.height)/2), withAttributes: attributes)
                context.flushGraphics()
                for p in 0..<576 { masks[i * 576 + p] = rep.bitmapData![p * 4 + 3] }
            }
            NSGraphicsContext.restoreGraphicsState()
        }
        glyphsReady = masks.withUnsafeBufferPointer { pet_set_glyphs($0.baseAddress, $0.count) }
    }
    @objc func checkUpdates() {
        guard !updateInFlight else { return }; updateInFlight = true
        Task { @MainActor in
            defer { updateInFlight = false }
            do {
                var request = URLRequest(url: URL(string: "https://api.github.com/repos/DLmember/Minecraft-Desktop-Pet/releases/latest")!)
                request.timeoutInterval = 12; request.setValue("MinecraftDesktopPet-macOS", forHTTPHeaderField: "User-Agent")
                request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
                let (data, response) = try await network.data(for: request)
                guard let http = response as? HTTPURLResponse, http.statusCode == 200, data.count <= 256 * 1024,
                      let value = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                      value["draft"] as? Bool == false, value["prerelease"] as? Bool == false,
                      let tag = value["tag_name"] as? String, StableVersion.parse(tag) != nil else { throw PetError("Invalid release") }
                let newer = StableVersion.newer(tag, than: appVersion)
                let assets = value["assets"] as? [[String: Any]] ?? []
                let hasMac = assets.contains { (($0["name"] as? String ?? "").lowercased().contains("mac")) }
                let a = NSAlert(); a.messageText = newer ? "发现新的正式版 \(tag)" : "当前没有更高的正式版本"
                a.informativeText = "当前：v\(appVersion) · 仓库：\(tag)\n" + (hasMac ? "发布页含 macOS 资源，请核对版本后下载。" : "该发布页尚未发现 macOS 安装包，请勿将 Windows 程序用于 macOS。")
                a.addButton(withTitle: "关闭"); a.addButton(withTitle: "打开发布页")
                if a.runModal() == .alertSecondButtonReturn {
                    NSWorkspace.shared.open(URL(string: "https://github.com/DLmember/Minecraft-Desktop-Pet/releases")!)
                }
            } catch { alert("Github又宕机了QAQ\n可能是网络异常或 API 限流，请稍后重试。") }
        }
    }
}
