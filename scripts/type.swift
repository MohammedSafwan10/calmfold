// Sends real keystrokes or scroll-wheel events to the focused window through
// the HID event tap, so they arrive exactly like a user's. For scripts/perf.sh.
// Needs Accessibility permission for the terminal.
//
//   swift scripts/type.swift text  <count> <interval-ms>   # types "word " patterns
//   swift scripts/type.swift keys  <count> <interval-ms> <keycode>
//   swift scripts/type.swift scroll <count> <interval-ms> <pixels>
// With KEEPSTONE_PID set, events go straight to that process (works even when
// its window is on another Space); otherwise to the HID tap.
import CoreGraphics
import Foundation

let args = CommandLine.arguments
guard args.count >= 4, let count = Int(args[2]), let interval = Double(args[3]) else {
    FileHandle.standardError.write("usage: type.swift text|keys|scroll <count> <interval-ms> [arg]\n".data(using: .utf8)!)
    exit(2)
}
let source = CGEventSource(stateID: .hidSystemState)
let delay = useconds_t(interval * 1000)
let pid = ProcessInfo.processInfo.environment["KEEPSTONE_PID"].flatMap { pid_t($0) }

func send(_ e: CGEvent?) {
    guard let e else { return }
    if let pid { e.postToPid(pid) } else { e.post(tap: .cghidEventTap) }
}

func key(_ code: CGKeyCode, char: String? = nil) {
    for down in [true, false] {
        let e = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down)
        if let char {
            let units = Array(char.utf16)
            e?.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
        }
        send(e)
    }
}

switch args[1] {
case "text":
    // Letters and spaces, like prose. Keycode 0 carries the unicode string.
    let pattern = Array("the quick brown fox jumps over a lazy dog ")
    for i in 0..<count {
        let c = String(pattern[i % pattern.count])
        key(c == " " ? 49 : 0, char: c)
        usleep(delay)
    }
case "keys":
    let code = CGKeyCode(args.count > 4 ? Int(args[4]) ?? 125 : 125)
    for _ in 0..<count {
        key(code)
        usleep(delay)
    }
case "scroll":
    // Wheel events go to the window under the pointer: park it over the note.
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    if let w = windows.first(where: {
        ($0[kCGWindowOwnerName as String] as? String)?.lowercased() == "keepstone" && ($0[kCGWindowLayer as String] as? Int) == 0
    }), let b = w[kCGWindowBounds as String] as? [String: Double] {
        CGWarpMouseCursorPosition(CGPoint(x: (b["X"] ?? 0) + (b["Width"] ?? 0) * 0.7, y: (b["Y"] ?? 0) + (b["Height"] ?? 0) * 0.5))
        usleep(200_000)
    }
    let pixels = Int32(args.count > 4 ? Int(args[4]) ?? -40 : -40)
    for _ in 0..<count {
        let e = CGEvent(scrollWheelEvent2Source: source, units: .pixel, wheelCount: 1, wheel1: pixels, wheel2: 0, wheel3: 0)
        send(e)
        usleep(delay)
    }
default:
    exit(2)
}
