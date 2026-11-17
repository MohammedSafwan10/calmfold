// Clicks at a point inside the Ottero window, in window-relative logical
// pixels (same coordinates as a 1x screenshot of the window). Needs
// Accessibility permission for the terminal. For manual/agent UI testing.
// Usage: swift scripts/click.swift <x> <y> [double]
import CoreGraphics
import Foundation

let args = CommandLine.arguments
guard args.count >= 3, let x = Double(args[1]), let y = Double(args[2]) else {
    FileHandle.standardError.write("usage: click.swift <x> <y> [double]\n".data(using: .utf8)!)
    exit(2)
}
let double = args.count > 3 && args[3] == "double"
let targetPID = ProcessInfo.processInfo.environment["OTTERO_WINDOW_PID"].flatMap(Int.init)

let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
// The main window is the largest layer-0 window (menu-bar helpers are 33 px tall).
func area(_ w: [String: Any]) -> Double {
    let b = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
    return (b["Width"] ?? 0) * (b["Height"] ?? 0)
}
guard let window = windows.filter({
    ($0[kCGWindowOwnerName as String] as? String)?.lowercased() == "ottero"
        && ($0[kCGWindowLayer as String] as? Int) == 0
        && (targetPID == nil || ($0[kCGWindowOwnerPID as String] as? Int) == targetPID)
}).max(by: { area($0) < area($1) }), let bounds = window[kCGWindowBounds as String] as? [String: Double] else {
    FileHandle.standardError.write("Ottero window not found on screen\n".data(using: .utf8)!)
    exit(1)
}

let point = CGPoint(x: (bounds["X"] ?? 0) + x, y: (bounds["Y"] ?? 0) + y)
let source = CGEventSource(stateID: .hidSystemState)
for click in 1...(double ? 2 : 1) {
    for type in [CGEventType.leftMouseDown, .leftMouseUp] {
        let event = CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: point, mouseButton: .left)
        event?.setIntegerValueField(.mouseEventClickState, value: Int64(click))
        event?.post(tap: .cghidEventTap)
        usleep(30_000)
    }
}
