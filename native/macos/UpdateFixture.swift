// Disposable native app for installed Sparkle upgrade tests. Never shipped.
import AppKit
import Foundation

final class Delegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        let info = Bundle.main.infoDictionary ?? [:]
        guard let log = info["FixtureLog"] as? String,
              let version = info["CFBundleVersion"] as? String else { exit(2) }
        let record = "\(version) \(ProcessInfo.processInfo.processIdentifier)\n"
        if !FileManager.default.fileExists(atPath: log) { _ = FileManager.default.createFile(atPath: log, contents: nil) }
        if let file = FileHandle(forWritingAtPath: log) {
            file.seekToEndOfFile(); file.write(Data(record.utf8)); try? file.close()
        }
    }
}
let application = NSApplication.shared
application.setActivationPolicy(.accessory)
let delegate = Delegate()
application.delegate = delegate
application.run()
