// Exits 0 if a Keepstone window is on screen in the current Space, 1 otherwise.
// Used by scripts/perf.sh: an off-screen window renders no frames, so
// measuring it would be meaningless.
import CoreGraphics

let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let visible = windows.contains {
    ($0[kCGWindowOwnerName as String] as? String)?.lowercased() == "keepstone"
        && ($0[kCGWindowLayer as String] as? Int) == 0
}
exit(visible ? 0 : 1)
