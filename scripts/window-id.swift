// Prints the CGWindowID of the first on-screen window owned by a process name.
// Used by scripts/screenshot.sh to capture only Ottero's window.
// Usage: swift scripts/window-id.swift ottero
import CoreGraphics
import Foundation

let owner = CommandLine.arguments.dropFirst().first ?? "ottero"
// Optional target when a disposable review app and the user's app both run.
let targetPID = ProcessInfo.processInfo.environment["OTTERO_WINDOW_PID"].flatMap(Int.init)
// Prefer on-screen windows; fall back to any window (e.g. on another Space).
let onScreen = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let all = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
// The main window is the largest layer-0 window (menu-bar helpers are 33 px tall).
func area(_ w: [String: Any]) -> Double {
    let b = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
    return (b["Width"] ?? 0) * (b["Height"] ?? 0)
}
func mine(_ w: [String: Any]) -> Bool {
    (w[kCGWindowOwnerName as String] as? String ?? "").lowercased() == owner.lowercased()
        && (w[kCGWindowLayer as String] as? Int) == 0
        && (targetPID == nil || (w[kCGWindowOwnerPID as String] as? Int) == targetPID)
}
for list in [onScreen, all] {
    if let window = list.filter(mine).max(by: { area($0) < area($1) }),
       let id = window[kCGWindowNumber as String] as? Int {
        print(id)
        exit(0)
    }
}
exit(1)
