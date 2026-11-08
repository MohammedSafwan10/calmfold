// Prints the CGWindowID of the first on-screen window owned by a process name.
// Used by scripts/screenshot.sh to capture only Keepstone's window.
// Usage: swift scripts/window-id.swift keepstone
import CoreGraphics
import Foundation

let owner = CommandLine.arguments.dropFirst().first ?? "keepstone"
// Prefer on-screen windows; fall back to any window (e.g. on another Space).
let onScreen = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let all = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
for window in onScreen + all.reversed() {
    let name = window[kCGWindowOwnerName as String] as? String ?? ""
    let layer = window[kCGWindowLayer as String] as? Int ?? -1
    if name.lowercased() == owner.lowercased(), layer == 0,
       let id = window[kCGWindowNumber as String] as? Int {
        print(id)
        exit(0)
    }
}
exit(1)
