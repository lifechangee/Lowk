// Lowk QA capture helper (macOS). Captures the app's OWN windows through ScreenCaptureKit with a
// per-window filter, and samples a window over time (frame stream + CGWindowList presence poll).
//
//   qa-cap list   <pid>
//   qa-cap shot   <pid> <overlay|main|pill> <out.png>
//   qa-cap sample <pid> <overlay|main|pill> <seconds> <out.json>
//
// IMPORTANT: the overlay window is `NSWindowSharingNone` (the stealth feature). ScreenCaptureKit - even
// with a per-window desktopIndependentWindow filter - does not deliver such a window while the
// exclusion is on (that is exactly what the stealth probe asserts). QA builds therefore lift the
// exclusion for the duration of a capture (`exclusion off` on the qa control channel). The
// presence sampler (CGWindowList metadata) does NOT need that: it reads window server metadata
// (on-screen flag, alpha, bounds), which stays available for protected windows.
import Foundation
import AppKit
import ScreenCaptureKit
import CoreGraphics
import CoreMedia
import ImageIO
import UniformTypeIdentifiers

func jsonOut(_ o: [String: Any]) { if let d = try? JSONSerialization.data(withJSONObject: o, options: [.sortedKeys]), let s = String(data: d, encoding: .utf8) { print(s) } }

func savePNG(_ img: CGImage, _ path: String) {
    guard let d = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, UTType.png.identifier as CFString, 1, nil) else { return }
    CGImageDestinationAddImage(d, img, nil); CGImageDestinationFinalize(d)
}

/// Fraction of pixels with any alpha (a vanished / blank window is fully transparent).
func opaqueFraction(_ img: CGImage) -> Double {
    let w = img.width, h = img.height
    guard w > 0, h > 0 else { return 0 }
    var buf = [UInt8](repeating: 0, count: w * h * 4)
    let cs = CGColorSpaceCreateDeviceRGB()
    buf.withUnsafeMutableBytes { p in
        let ctx = CGContext(data: p.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
    }
    var n = 0, tot = 0, i = 3
    while i < buf.count { if buf[i] > 8 { n += 1 }; tot += 1; i += 4 * 7 }
    return Double(n) / Double(max(tot, 1))
}

func pick(_ wins: [SCWindow], _ pid: pid_t, _ which: String) -> SCWindow? {
    let mine = wins.filter { $0.owningApplication?.processID == pid }
    switch which {
    case "overlay": return mine.first { $0.title == "Lowk Overlay" } ?? mine.first { abs($0.frame.width - 600) < 2 && abs($0.frame.height - 384) < 2 }
    case "pill": return mine.first { $0.frame.height <= 30 || $0.frame.width <= 30 }
    default: return mine.filter { $0.title != "Lowk Overlay" && $0.frame.width > 300 }.max { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }
    }
}

func shareable() async throws -> [SCWindow] {
    try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false).windows
}

/// CGWindowList entry for the same window (metadata only).
func cgInfo(_ id: CGWindowID) -> [String: Any]? {
    (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]])?.first { ($0[kCGWindowNumber as String] as? Int) == Int(id) }
}

final class Frames: NSObject, SCStreamOutput {
    var frames = 0, blank = 0, idle = 0
    var firstBlankAt: Double? = nil
    let t0 = Date()
    var last: CGImage?
    let ci = CIContext()
    func stream(_ s: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of t: SCStreamOutputType) {
        guard t == .screen else { return }
        if let arr = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]], let st = arr.first?[.status] as? Int {
            if st == SCFrameStatus.idle.rawValue { idle += 1; return }
            if st != SCFrameStatus.complete.rawValue { frames += 1; blank += 1; if firstBlankAt == nil { firstBlankAt = Date().timeIntervalSince(t0) }; return }
        }
        guard let pb = CMSampleBufferGetImageBuffer(sb) else { return }
        let img = ci.createCGImage(CIImage(cvPixelBuffer: pb), from: CIImage(cvPixelBuffer: pb).extent)
        frames += 1
        if let img = img {
            last = img
            if opaqueFraction(img) < 0.02 { blank += 1; if firstBlankAt == nil { firstBlankAt = Date().timeIntervalSince(t0) } }
        }
    }
}

let a = CommandLine.arguments
guard a.count >= 3, let pid32 = Int32(a[2]) else { print("usage: qa-cap list|shot|sample <pid> ..."); exit(2) }
let pid = pid32

switch a[1] {
case "list":
    let rows = (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []).filter { ($0[kCGWindowOwnerPID as String] as? Int) == Int(pid) }.map { w -> [String: Any] in
        ["id": w[kCGWindowNumber as String] ?? 0, "name": w[kCGWindowName as String] ?? "", "onscreen": w[kCGWindowIsOnscreen as String] ?? false, "sharing": w[kCGWindowSharingState as String] ?? -1, "alpha": w[kCGWindowAlpha as String] ?? 1, "bounds": w[kCGWindowBounds as String] ?? [:]]
    }
    jsonOut(["windows": rows]); exit(0)

case "shot" where a.count >= 5:
    do {
        guard let w = pick(try await shareable(), pid, a[3]) else { jsonOut(["ok": false, "error": "window '\(a[3])' not found by ScreenCaptureKit (hidden, or excluded: lift the exclusion for QA)"]); exit(1) }
        let cfg = SCStreamConfiguration()
        let scale = NSScreen.main?.backingScaleFactor ?? 2
        cfg.width = Int(w.frame.width * scale); cfg.height = Int(w.frame.height * scale); cfg.showsCursor = false; cfg.scalesToFit = true
        let img: CGImage = try await SCScreenshotManager.captureImage(contentFilter: SCContentFilter(desktopIndependentWindow: w), configuration: cfg)
        savePNG(img, a[4])
        let frac = opaqueFraction(img)
        jsonOut(["ok": frac > 0.02, "w": img.width, "h": img.height, "opaque": frac, "path": a[4], "error": frac > 0.02 ? "" : "captured image is blank (window excluded from capture?)"])
        exit(frac > 0.02 ? 0 : 1)
    } catch { jsonOut(["ok": false, "error": "\(error)"]); exit(1) }

case "sample" where a.count >= 6:
    let secs = Double(a[4]) ?? 3
    do {
        guard let w = pick(try await shareable(), pid, a[3]) else { jsonOut(["ok": false, "error": "window '\(a[3])' not found"]); exit(1) }
        let id = w.windowID
        let cfg = SCStreamConfiguration()
        cfg.width = Int(w.frame.width * 2); cfg.height = Int(w.frame.height * 2); cfg.showsCursor = false
        cfg.minimumFrameInterval = CMTime(value: 1, timescale: 60); cfg.queueDepth = 8
        let sink = Frames()
        let stream = SCStream(filter: SCContentFilter(desktopIndependentWindow: w), configuration: cfg, delegate: nil)
        try stream.addStreamOutput(sink, type: .screen, sampleHandlerQueue: DispatchQueue(label: "frames"))
        try await stream.startCapture()
        // Presence poll: window server metadata at ~250 Hz.
        var samples = 0, absent = 0
        var absentRuns: [[String: Double]] = []
        var runStart: Double? = nil
        let t0 = Date()
        while Date().timeIntervalSince(t0) < secs {
            let t = Date().timeIntervalSince(t0)
            let info = cgInfo(id)
            let on = (info?[kCGWindowIsOnscreen as String] as? Bool) ?? false
            let alpha = (info?[kCGWindowAlpha as String] as? Double) ?? 0
            samples += 1
            if !on || alpha < 0.01 { absent += 1; if runStart == nil { runStart = t } }
            else if let s = runStart { absentRuns.append(["from": s, "to": t]); runStart = nil }
            try await Task.sleep(nanoseconds: 4_000_000)
        }
        if let s = runStart { absentRuns.append(["from": s, "to": secs]) }
        try? await stream.stopCapture()
        let fps = Double(sink.frames + sink.idle) / secs
        jsonOut(["ok": absent == 0 && sink.blank == 0, "seconds": secs, "presenceSamples": samples, "presenceHz": Double(samples) / secs, "absentSamples": absent, "absentRuns": absentRuns,
                 "frames": sink.frames, "idleFrames": sink.idle, "blankFrames": sink.blank, "streamFps": fps, "firstBlankAt": sink.firstBlankAt ?? -1])
        if let l = sink.last { savePNG(l, (a[5] as NSString).deletingPathExtension + "-last.png") }
        try? FileManager.default.removeItem(atPath: a[5] + ".tmp")
        exit(absent == 0 && sink.blank == 0 ? 0 : 1)
    } catch { jsonOut(["ok": false, "error": "\(error)"]); exit(1) }

default: print("usage: qa-cap list <pid> | shot <pid> <overlay|main|pill> <png> | sample <pid> <win> <secs> <json>"); exit(2)
}
