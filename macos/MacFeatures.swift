import AppKit
import CoreAudio

/// The players whose output shows music notes. Helper processes (for example
/// Chromium-based audio services) carry the app's id plus a suffix.
/// Mirrors the Windows 2.5.1 player list where a macOS build exists. Ids come from
/// the App Store and Homebrew cask metadata. Browsers and video players never count.
enum MusicPlayers {
    static let bundleIDs = [
        "com.netease.163music", "com.tencent.QQMusicMac", "com.apple.Music", "com.spotify.client",
        "com.kugou.mac.Music", "com.wenyu.kwplayermac", "com.soda.music",
        "com.kkbox.KKBOX", "com.kkbox.electron-app", "com.listen1.listen1", "com.electron.yesplaymusic",
        "com.amazon.music", "com.amazon.music-renderer", "com.deezer.deezer-desktop", "com.tidal.desktop",
        "com.qobuz.desktop", "com.qobuz.QobuzDesktop", "com.pandora.desktop", "tv.plex.plexamp",
        "com.audirvana.Audirvana", "com.audirvana.Audirvana-Plus", "com.foobar2000.mac",
        "com.coppertino.Vox", "com.swinsian.Swinsian", "org.clementine-player.clementine",
    ]
    static func matches(_ bundleID: String?) -> Bool {
        guard let id = bundleID?.lowercased(), !id.isEmpty else { return false }
        return base(bundleID) != nil
    }
    /// The supported player a process belongs to, helper processes included.
    static func base(_ bundleID: String?) -> String? {
        guard let id = bundleID?.lowercased(), !id.isEmpty else { return nil }
        return bundleIDs.first { id == $0.lowercased() || id.hasPrefix($0.lowercased() + ".") }
    }
    /// Players that broadcast play/pause as a distributed notification. A paused
    /// player may keep its output stream open and send silence, so this state wins.
    static let stateNotifications = ["com.apple.Music.playerInfo": "com.apple.Music",
                                     "com.spotify.client.PlaybackStateChanged": "com.spotify.client"]
    static func reportedPlaying(_ userInfo: [AnyHashable: Any]?) -> Bool? {
        guard let state = userInfo?["Player State"] as? String else { return nil }
        return state == "Playing"
    }
}

/// Latest play/pause state broadcast by Apple Music and Spotify. Main thread only.
final class PlayerStateWatcher {
    private(set) var reported: [String: Bool] = [:]
    var onChange: (() -> Void)?
    private var tokens: [NSObjectProtocol] = []
    func start() {
        guard tokens.isEmpty else { return }
        let center = DistributedNotificationCenter.default()
        for (name, player) in MusicPlayers.stateNotifications {
            tokens.append(center.addObserver(forName: Notification.Name(name), object: nil, queue: .main) { [weak self] note in
                guard let self, let playing = MusicPlayers.reportedPlaying(note.userInfo) else { return }
                self.reported[player] = playing; self.onChange?()
            })
        }
        // A quit or crashed player sends nothing more; forget what it said.
        tokens.append(NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.didTerminateApplicationNotification,
                                                                        object: nil, queue: .main) { [weak self] note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            guard let self, let player = MusicPlayers.base(app?.bundleIdentifier), self.reported[player] != nil else { return }
            self.reported[player] = nil; self.onChange?()
        })
    }
    func stop() {
        for token in tokens {
            DistributedNotificationCenter.default().removeObserver(token)
            NSWorkspace.shared.notificationCenter.removeObserver(token)
        }
        tokens.removeAll(); reported.removeAll()
    }
}

/// Read HAL activity flags and process bundle ids only. No audio tap, samples,
/// microphone, Apple Events or private framework is involved.
enum AudioOutputProbe {
    struct Snapshot {
        let playing: Bool?
        let source: String
        var players: [String] = []
        /// Supported player processes: "bundle id pid=… output=…", for diagnostics.
        var processes: [String] = []
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
    static func string(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector) -> String? {
        var property = address(selector), value: Unmanaged<CFString>?
        var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
        let status = withUnsafeMutablePointer(to: &value) { AudioObjectGetPropertyData(object, &property, 0, nil, &size, $0) }
        guard status == noErr, let value else { return nil }
        return value.takeRetainedValue() as String
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
    /// Output state of the supported players only; other apps are ignored. A
    /// broadcast play/pause state replaces that player's stream state.
    static func playerActivity(_ clients: [(bundleID: String?, running: UInt32?)], reported: [String: Bool] = [:]) -> Bool? {
        if reported.values.contains(true) { return true }
        return activity(clients.compactMap { client in
            MusicPlayers.base(client.bundleID).flatMap { reported[$0] == nil ? .some(client.running) : nil }
        })
    }
    /// macOS 12–14.1: open players without a broadcast state, plus any running output device.
    static func legacyActivity(open: [String], deviceRunning: Bool?, reported: [String: Bool]) -> Bool? {
        if reported.values.contains(true) { return true }
        return open.contains { MusicPlayers.base($0).map { reported[$0] == nil } ?? false } ? deviceRunning : false
    }
    static func snapshot(reported: [String: Bool] = [:]) -> Snapshot {
        let system = AudioObjectID(kAudioObjectSystemObject)
        // These selectors appeared with process objects in macOS 14.2. The
        // runtime property check also tolerates an unavailable/restarting HAL.
        if #available(macOS 14.2, *),
           let processes = objects(system, kAudioHardwarePropertyProcessObjectList) {
            let own = UInt32(getpid())
            var lines: [String] = []
            let clients = processes.compactMap { process -> (bundleID: String?, running: UInt32?)? in
                // A process may exit between enumeration and reading its state.
                guard let pid = word(process, kAudioProcessPropertyPID), pid != own else { return nil }
                let id = string(process, kAudioProcessPropertyBundleID).flatMap { $0.isEmpty ? nil : $0 }
                    ?? NSRunningApplication(processIdentifier: pid_t(pid))?.bundleIdentifier
                let running = word(process, kAudioProcessPropertyIsRunningOutput)
                if MusicPlayers.matches(id) { lines.append("\(id!) pid=\(pid) output=\(running.map(String.init) ?? "?")") }
                return (id, running)
            }
            let streaming = clients.filter { $0.running == 1 }.compactMap { MusicPlayers.base($0.bundleID) }
            let players = Set(streaming.filter { reported[$0] == nil } + reported.filter(\.value).map(\.key)).sorted()
            return Snapshot(playing: playerActivity(clients, reported: reported), source: "process-output",
                            players: players, processes: lines)
        }
        // Compatibility path for macOS 12/13, which has no per-process list: a
        // running output device while a supported player is open counts.
        let open = NSWorkspace.shared.runningApplications.compactMap(\.bundleIdentifier).filter(MusicPlayers.matches)
        if reported.values.contains(true) || open.isEmpty {
            let playing = legacyActivity(open: open, deviceRunning: false, reported: reported)
            return Snapshot(playing: playing, source: "output-device", players: reported.filter(\.value).map(\.key).sorted())
        }
        guard let devices = objects(system, kAudioHardwarePropertyDevices) else {
            return Snapshot(playing: nil, source: "unavailable")
        }
        var states: [UInt32?] = []
        for device in devices {
            guard let streams = objects(device, kAudioDevicePropertyStreams, scope: kAudioObjectPropertyScopeOutput),
                  !streams.isEmpty else { continue }
            states.append(word(device, kAudioDevicePropertyDeviceIsRunningSomewhere))
        }
        let playing = legacyActivity(open: open, deviceRunning: activity(states), reported: reported)
        let unreported = Set(open.compactMap(MusicPlayers.base).filter { reported[$0] == nil }).sorted()
        return Snapshot(playing: playing, source: "output-device", players: playing == true ? unreported : [])
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
    private let players = PlayerStateWatcher()
    func setEnabled(_ enabled: Bool) {
        guard enabled != self.enabled else { return }
        generation += 1; self.enabled = enabled
        lastPlaying = nil; lastMessage = ""
        timer?.invalidate(); timer = nil
        guard enabled else { players.stop(); onChange?(false, "音乐音符已关闭。"); return }
        players.onChange = { [weak self] in self?.poll() }
        players.start()
        poll()
        let t = Timer(timeInterval: 1, repeats: true) { [weak self] _ in self?.poll() }
        t.tolerance = 0.2
        RunLoop.main.add(t, forMode: .common); timer = t
    }
    func stop() { generation += 1; enabled = false; timer?.invalidate(); timer = nil; players.stop() }
    private func poll() {
        guard enabled, !busy else { return }
        busy = true; let token = generation, reported = players.reported
        queue.async { [weak self] in
            // Bundle-id lookups autorelease; drain them on every poll.
            let state = autoreleasepool { AudioOutputProbe.snapshot(reported: reported) }
            let message: String
            switch state.playing {
            case true?: message = "检测到音乐播放。"
            case false?: message = "等待支持的音乐软件播放（网易云、QQ、酷狗、酷我、汽水、Apple Music、Spotify 等）。"
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
        musicToggle?.toolTip = musicStatus + " 仅支持的音乐软件播放时显示音符，浏览器和视频播放器不会触发。"
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
