// Prints the CGWindowID of the first on-screen window owned by a process name.
// Used by scripts/screenshot.sh to capture only Calmfold's window.
// Usage: swift scripts/window-id.swift calmfold
import CoreGraphics
import Foundation

let owner = CommandLine.arguments.dropFirst().first ?? "calmfold"
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
}
for list in [onScreen, all] {
    if let window = list.filter(mine).max(by: { area($0) < area($1) }),
       let id = window[kCGWindowNumber as String] as? Int {
        print(id)
        exit(0)
    }
}
exit(1)
