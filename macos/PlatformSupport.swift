import Foundation
import Darwin

let appVersion = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "development"

enum LaunchMode {
    /// "--autostart" comes from our LaunchAgent; a System Settings login item
    /// carries keyAELaunchedAsLogInItem in its open-application event instead.
    static func loginArguments(_ arguments: [String], launchedAsLoginItem: Bool) -> [String] {
        launchedAsLoginItem && !arguments.contains("--autostart") ? arguments + ["--autostart"] : arguments
    }
    /// With both our LaunchAgent and a login item, the second launch at login
    /// reaches the running app as a reopen event. It must not open the console.
    static func ignoresReopen(arguments: [String], secondsSinceLaunch: Double, delay: Int) -> Bool {
        arguments.contains("--autostart") && secondsSinceLaunch < Double(delay) + 30
    }
    static func shouldShowSettings(arguments: [String], keyboardEnabled: Bool,
                                   keyboardRunning: Bool, quickChatEnabled: Bool,
                                   quickChatReady: Bool) -> Bool {
        // Login launches remain quiet, including when input permissions are missing.
        guard !arguments.contains("--autostart") else { return false }
        return arguments.contains("--show-settings")
            || (keyboardEnabled && !keyboardRunning)
            || (quickChatEnabled && !quickChatReady)
    }
}

/// What the input-permission window reports: green once keyboard capture and
/// Accessibility both work, yellow with one of them, red with neither.
enum PermissionLook {
    enum Level { case green, yellow, red }
    static func level(inputOK: Bool, access: Bool) -> Level {
        inputOK && access ? .green : (inputOK || access ? .yellow : .red)
    }
    /// Problems first line; `nil` when both items work.
    static func problem(listen: Bool, access: Bool, inputOK: Bool) -> String? {
        var parts: [String] = []
        if !inputOK {
            parts.append(listen ? "系统显示输入监控已授权，但键盘仍未连接：多半是重装前留下的旧授权记录。"
                                : "输入监控未授权：角色只能在本应用窗口内敲字。")
        }
        if !access { parts.append("辅助功能未授权：快速聊天不可用。") }
        return parts.isEmpty ? nil : parts.joined(separator: " ")
    }
    /// Input Monitoring is satisfied once global keys actually arrive.
    static func inputSatisfied(listen: Bool, keyboardEnabled: Bool, keyboardRunning: Bool) -> Bool {
        keyboardEnabled ? keyboardRunning : listen
    }
}

/// The login agent exits after handing off to LaunchServices. No KeepAlive and
/// no bootstrap/bootout on edits: registration never kills or relaunches the app.
enum LoginStartup {
    static let label = "com.dlmember.minecraft-desktop-pet.macos.login"
    static let delays = [0, 5, 15, 30, 60, 120]
    static func delay(arguments: [String], enabled: Bool, seconds: Int) -> Int {
        arguments.contains("--autostart") && enabled && delays.contains(seconds) ? seconds : 0
    }
    static func propertyList(app: URL, data: URL) -> [String: Any] {
        ["Label": label, "ProgramArguments": ["/usr/bin/open", "-g", "-a", app.path,
            "--args", "--autostart", "--data-dir", data.path], "RunAtLoad": true,
         "LimitLoadToSessionType": "Aqua", "ProcessType": "Background"]
    }
    static func setEnabled(_ enabled: Bool, app: URL, data: URL, directory: URL) throws {
        let target = directory.appendingPathComponent(label + ".plist")
        if enabled {
            guard app.pathExtension == "app", FileManager.default.fileExists(atPath: app.appendingPathComponent("Contents/MacOS/MinecraftPet").path) else {
                throw PetError("请从完整的 .app 应用中设置登录启动。")
            }
            let bytes = try PropertyListSerialization.data(fromPropertyList: propertyList(app: app, data: data), format: .xml, options: 0)
            if (try? Data(contentsOf: target)) == bytes { return }
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            try bytes.write(to: target, options: .atomic)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: target.path)
        } else if FileManager.default.fileExists(atPath: target.path) {
            let bytes = try Data(contentsOf: target)
            let plist = try PropertyListSerialization.propertyList(from: bytes, format: nil) as? [String: Any]
            guard let args = plist?["ProgramArguments"] as? [String],
                  let i = args.firstIndex(of: "--data-dir"), args.indices.contains(i+1), args[i+1] == data.path else {
                throw PetError("登录启动项属于另一个设置目录，未修改。")
            }
            try FileManager.default.removeItem(at: target)
        }
    }
}

enum OfficialTexture {
    static func address(_ value: String) throws -> String {
        guard let url = URLComponents(string: value),
              ["http", "https"].contains(url.scheme ?? ""), url.host == "textures.minecraft.net",
              url.user == nil, url.password == nil, url.port == nil, url.query == nil, url.fragment == nil,
              url.path.hasPrefix("/texture/") else { throw PetError("官方纹理地址无效。") }
        let hash = String(url.path.dropFirst("/texture/".count))
        guard !hash.isEmpty, hash.count <= 128, hash.unicodeScalars.allSatisfy({ CharacterSet(charactersIn: "0123456789abcdefABCDEF").contains($0) }) else {
            throw PetError("官方纹理地址无效。")
        }
        return "https://textures.minecraft.net/texture/\(hash)"
    }
}

final class OfficialSessionDelegate: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        let allowed = request.url?.scheme == "https" && request.url?.host == task.originalRequest?.url?.host
        completionHandler(allowed ? request : nil)
    }
}
