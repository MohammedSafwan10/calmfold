// Sparkle owns networking, verification, extraction, installation and relaunch.
// This UI-free bridge only maps its documented user-driver callbacks to GPUI.
// No WebKit, signing identity or Keychain access is used here.
import AppKit
import Foundation
import Sparkle
import Darwin

@MainActor
final class Driver: NSObject, SPUUserDriver, SPUUpdaterDelegate {
    var updater: SPUUpdater!
    var version = ""
    var received: UInt64 = 0
    var total: UInt64 = 0
    var cancel: (() -> Void)?
    var installReply: ((SPUUserUpdateChoice) -> Void)?
    var installNow: (() -> Void)?
    var installing = false
    var pinnedFeed: String?
    var expectedVersion: String?
    func failure(_ error: Error) {
        let outer = error as NSError
        #if UPDATER_TESTING
        var cause = outer
        var messages = [outer.localizedDescription]
        for _ in 0..<4 {
            guard let next = cause.userInfo[NSUnderlyingErrorKey] as? NSError else { break }
            messages.append(next.localizedDescription); cause = next
        }
        emit("failed", ["message": messages.joined(separator: " "), "code": outer.code])
        #else
        emit("failed", ["message": outer.localizedDescription, "code": outer.code])
        #endif
    }

    func emit(_ event: String, _ fields: [String: Any] = [:]) {
        var body = fields; body["event"] = event
        guard let data = try? JSONSerialization.data(withJSONObject: body, options: [.sortedKeys]) else { return }
        try? FileHandle.standardOutput.write(contentsOf: data + Data([10]))
    }
    func command(_ line: String) {
        if line.hasPrefix("check ") {
            let parts = line.split(separator: " ")
            guard parts.count == 3, let url = URL(string: String(parts[1])),
                  url.user == nil, url.password == nil else { return }
            #if UPDATER_TESTING
            guard url.scheme == "https" || (url.scheme == "http" && url.host == "127.0.0.1") else { return }
            #else
            guard url.scheme == "https" else { return }
            #endif
            pinnedFeed = String(parts[1]); expectedVersion = String(parts[2])
            command("check"); return
        }
        switch line {
        case "check":
            if updater.canCheckForUpdates { emit("checking"); updater.checkForUpdates() }
        case "install":
            installing = true
            if let reply = installReply { installReply = nil; reply(.install) }
            else if let block = installNow { installNow = nil; block() }
            else { installing = false; emit("failed", ["message": "No verified update is ready."]) }
        case "cancel": cancel?()
        default: break
        }
    }
    func inputClosed() {
        // Once installation is authorized, Sparkle outlives the host and owns
        // the remainder of the transaction. Otherwise tear down this session.
        if !installing { cancel?(); installReply?(.skip); exit(0) }
    }
    func show(_ request: SPUUpdatePermissionRequest, reply: @escaping (SUUpdatePermissionResponse) -> Void) {
        reply(SUUpdatePermissionResponse(automaticUpdateChecks: false, sendSystemProfile: false))
    }
    func showUserInitiatedUpdateCheck(cancellation: @escaping () -> Void) { cancel = cancellation; emit("checking") }
    func showUpdateFound(with appcastItem: SUAppcastItem, state: SPUUserUpdateState, reply: @escaping (SPUUserUpdateChoice) -> Void) {
        version = appcastItem.displayVersionString
        if let expectedVersion, version != expectedVersion {
            reply(.dismiss); emit("failed", ["message": "The update catalogs disagree. Try again later."]); return
        }
        if appcastItem.isInformationOnlyUpdate { reply(.dismiss); emit("failed", ["message": "This update requires a manual installation."]); return }
        emit("found", ["version": version])
        if state.stage == .installing { installReply = reply; emit("ready", ["version": version]) }
        else { reply(.install) }
    }
    func showUpdateReleaseNotes(with downloadData: SPUDownloadData) {}
    func showUpdateReleaseNotesFailedToDownloadWithError(_ error: Error) {}
    func showUpdateNotFoundWithError(_ error: Error, acknowledgement: @escaping () -> Void) { emit("up_to_date"); acknowledgement() }
    func showUpdaterError(_ error: Error, acknowledgement: @escaping () -> Void) {
        failure(error); acknowledgement()
    }
    func showDownloadInitiated(cancellation: @escaping () -> Void) { cancel = cancellation; received = 0; total = 0; emit("downloading", ["version": version]) }
    func showDownloadDidReceiveExpectedContentLength(_ expectedContentLength: UInt64) { total = expectedContentLength }
    func showDownloadDidReceiveData(ofLength length: UInt64) { received += length }
    func showDownloadDidStartExtractingUpdate() { cancel = nil; emit("verifying", ["version": version]) }
    func showExtractionReceivedProgress(_ progress: Double) {}
    func showReady(toInstallAndRelaunch reply: @escaping (SPUUserUpdateChoice) -> Void) {
        installReply = reply; emit("ready", ["version": version])
    }
    func showInstallingUpdate(withApplicationTerminated applicationTerminated: Bool, retryTerminatingApplication: @escaping () -> Void) {
        emit("installing", ["version": version])
    }
    func showUpdateInstalledAndRelaunched(_ relaunched: Bool, acknowledgement: @escaping () -> Void) {
        emit("installed", ["relaunched": relaunched]); acknowledgement(); exit(0)
    }
    func dismissUpdateInstallation() { cancel = nil; emit("settled") }
    func updater(_ updater: SPUUpdater, shouldDownloadReleaseNotesForUpdate updateItem: SUAppcastItem) -> Bool { false }
    func feedURLString(for updater: SPUUpdater) -> String? { pinnedFeed }
    func updater(_ updater: SPUUpdater, didAbortWithError error: Error) {
        failure(error)
    }
    func updater(_ updater: SPUUpdater, willInstallUpdateOnQuit item: SUAppcastItem, immediateInstallationBlock: @escaping () -> Void) -> Bool {
        version = item.displayVersionString; installNow = immediateInstallationBlock
        emit("ready", ["version": version]); return true
    }
}

@main @MainActor
struct Main {
    static func main() {
        signal(SIGPIPE, SIG_IGN)
        do {
            guard CommandLine.arguments.count == 2, let host = Bundle(path: CommandLine.arguments[1]),
                  let feed = host.object(forInfoDictionaryKey: "SUFeedURL") as? String,
                  let url = URL(string: feed) else { throw NSError(domain: "OtteroUpdater", code: 1) }
            #if UPDATER_TESTING
            guard url.scheme == "https" || (url.scheme == "http" && url.host == "127.0.0.1") else { throw NSError(domain: "OtteroUpdater", code: 2) }
            #else
            guard url.scheme == "https" else { throw NSError(domain: "OtteroUpdater", code: 2) }
            #endif
            _ = NSApplication.shared
            NSApp.setActivationPolicy(.prohibited)
            let driver = Driver()
            let updater = SPUUpdater(hostBundle: host, applicationBundle: host, userDriver: driver, delegate: driver)
            driver.updater = updater
            updater.automaticallyChecksForUpdates = false
            updater.automaticallyDownloadsUpdates = false
            try updater.start()
            driver.emit("initialized")
            DispatchQueue.global(qos: .utility).async {
                // Do not use readLine(): it holds libc's stdin FILE lock while
                // waiting, which deadlocks Foundation's XPath parser in Sparkle.
                var pending = Data()
                while true {
                    let chunk = FileHandle.standardInput.availableData
                    if chunk.isEmpty { break }
                    pending.append(chunk)
                    if pending.count > 1024 { break }
                    while let newline = pending.firstIndex(of: 10) {
                        let line = String(decoding: pending[..<newline], as: UTF8.self)
                        pending.removeSubrange(...newline)
                        DispatchQueue.main.async { driver.command(line) }
                    }
                }
                DispatchQueue.main.async { driver.inputClosed() }
            }
            // Sparkle waits for NSApplication's launch lifecycle; a bare
            // Foundation RunLoop never posts applicationDidFinishLaunching.
            withExtendedLifetime(driver) { NSApp.run() }
        } catch {
            let body: [String: Any] = ["event": "failed", "message": "The native updater could not initialize."]
            if let data = try? JSONSerialization.data(withJSONObject: body) { FileHandle.standardOutput.write(data + Data([10])) }
            exit(1)
        }
    }
}
