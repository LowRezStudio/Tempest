// The largest on-screen, normal-layer window of a process: its window number
// for `screencapture -l` (captures that window only).
import CoreGraphics
import Foundation

let pid = Int32(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
var best: (Int, Double) = (0, 0)
for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == pid && (w[kCGWindowLayer as String] as? Int) == 0 {
    let b = w[kCGWindowBounds as String] as! [String: Double]
    let area = (b["Width"] ?? 0) * (b["Height"] ?? 0)
    if area > best.1 { best = (w[kCGWindowNumber as String] as! Int, area) }
}
print(best.0)
