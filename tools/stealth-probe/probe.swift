// THROWAWAY SPIKE probe: captures the screen 4 ways and reports marker presence.
// usage: probe <outdir> <label> [--owner NAME] [--expect overlay=hidden|visible,main=hidden|visible|any]  |  probe fsdummy
// Exit code: 0 all expectations met, 1 an expectation failed or a capture method errored, 2 usage.
import Foundation
import AppKit
import ScreenCaptureKit
import CoreGraphics
import CoreMedia
import ImageIO
import UniformTypeIdentifiers

func savePNG(_ img: CGImage, _ path: String) {
    guard let d = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, UTType.png.identifier as CFString, 1, nil) else { return }
    CGImageDestinationAddImage(d, img, nil); CGImageDestinationFinalize(d)
}

// Count marker pixels. overlay = magenta (r,b high, g low); main = green (g high, r,b low)
func count(_ img: CGImage) -> (Int, Int) {
    let w = img.width, h = img.height
    var buf = [UInt8](repeating: 0, count: w * h * 4)
    let cs = CGColorSpaceCreateDeviceRGB()
    buf.withUnsafeMutableBytes { p in
        let ctx = CGContext(data: p.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: cs,
                            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
    }
    var o = 0, m = 0
    var i = 0
    while i < buf.count {
        let r = Int(buf[i]), g = Int(buf[i+1]), b = Int(buf[i+2])
        if r > 170 && b > 170 && g < 100 { o += 1 }
        else if g > 220 && r < 60 && b < 60 { m += 1 }
        i += 4
    }
    return (o, m)
}

var ownerFilter = "fixture"
struct Row { let method: String; let ok: Bool; let overlay: Int; let main: Int; let note: String }
var rows: [Row] = []
func record(_ method: String, _ img: CGImage?, _ outdir: String, _ label: String, note: String = "") {
    guard let img = img else { rows.append(Row(method: method, ok: false, overlay: 0, main: 0, note: note)); return }
    savePNG(img, "\(outdir)/\(label)-\(method).png")
    let (o, m) = count(img)
    rows.append(Row(method: method, ok: true, overlay: o, main: m, note: note + " \(img.width)x\(img.height)"))
}

final class OneFrame: NSObject, SCStreamOutput {
    var img: CGImage?; let sem = DispatchSemaphore(value: 0)
    func stream(_ s: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of t: SCStreamOutputType) {
        guard t == .screen, img == nil, let pb = CMSampleBufferGetImageBuffer(sb) else { return }
        // skip frames without content (status != complete)
        if let arr = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
           let st = arr.first?[.status] as? Int, st != SCFrameStatus.complete.rawValue { return }
        let ci = CIImage(cvPixelBuffer: pb)
        img = CIContext().createCGImage(ci, from: ci.extent); sem.signal()
    }
}

func runProbe(outdir: String, label: String, expect: [String: String]) async -> Bool {
    try? FileManager.default.createDirectory(atPath: outdir, withIntermediateDirectories: true)
    print("preflight screen capture access: \(CGPreflightScreenCaptureAccess())")
    // window list info on overlay
    if let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] {
        for w in list {
            let name = (w[kCGWindowName as String] as? String) ?? ""
            let owner = (w[kCGWindowOwnerName as String] as? String) ?? ""
            if owner.contains(ownerFilter) {
                print("  CGWindow owner=\(owner) name='\(name)' sharing=\(w[kCGWindowSharingState as String] ?? "?") layer=\(w[kCGWindowLayer as String] ?? "?") onscreen=\(w[kCGWindowIsOnscreen as String] ?? "?") bounds=\(w[kCGWindowBounds as String] ?? "?")")
            }
        }
    }
    var display: SCDisplay?; var content: SCShareableContent?
    do {
        content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
        display = content?.displays.first
        for w in content?.windows ?? [] where (w.owningApplication?.applicationName ?? "").contains(ownerFilter) {
            print("  SCWindow id=\(w.windowID) title='\(w.title ?? "")' layer=\(w.windowLayer) onScreen=\(w.isOnScreen) frame=\(w.frame)")
        }
    } catch { print("SCShareableContent error: \(error)") }

    if let d = display {
        let cfg = SCStreamConfiguration()
        cfg.width = d.width * 2; cfg.height = d.height * 2; cfg.showsCursor = false
        let filter = SCContentFilter(display: d, excludingWindows: [])
        // (a)
        do { let img = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: cfg)
             record("a_sck_screenshot", img, outdir, label) }
        catch { record("a_sck_screenshot", nil, outdir, label, note: "\(error)") }
        // (b)
        do {
            let s = SCStream(filter: filter, configuration: cfg, delegate: nil)
            let of = OneFrame()
            try s.addStreamOutput(of, type: .screen, sampleHandlerQueue: DispatchQueue(label: "f"))
            try await s.startCapture()
            for _ in 0..<50 { if of.img != nil { break }; try await Task.sleep(nanoseconds: 100_000_000) }
            try await s.stopCapture()
            record("b_sck_stream", of.img, outdir, label, note: of.img == nil ? "no frame in 5s" : "")
        } catch { record("b_sck_stream", nil, outdir, label, note: "\(error)") }
    } else { record("a_sck_screenshot", nil, outdir, label, note: "no display"); record("b_sck_stream", nil, outdir, label, note: "no display") }

    // (c) legacy CGWindowListCreateImage via dlsym (unavailable at compile time on new SDKs)
    typealias Fn = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?
    if let sym = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImage") { // RTLD_DEFAULT
        let f = unsafeBitCast(sym, to: Fn.self)
        // CGRectInfinite-like null rect, onScreenOnly=1, kCGNullWindowID=0, bestResolution=1<<3 (kCGWindowImageBestResolution)
        let img = f(CGRect.null, 1 << 0, 0, 1 << 3)?.takeRetainedValue()
        record("c_cgwindowlist", img, outdir, label, note: img == nil ? "returned nil (blocked/removed)" : "")
    } else { record("c_cgwindowlist", nil, outdir, label, note: "symbol not found") }

    // (d) screencapture CLI
    let path = "\(outdir)/\(label)-d_screencapture_cli.png"
    try? FileManager.default.removeItem(atPath: path)
    let p = Process(); p.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture"); p.arguments = ["-x", "-t", "png", path]
    try? p.run(); p.waitUntilExit()
    if let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil), let img = CGImageSourceCreateImageAtIndex(src, 0, nil) {
        record("d_screencapture_cli", img, outdir, label)
    } else { record("d_screencapture_cli", nil, outdir, label, note: "no file, exit \(p.terminationStatus)") }

    print("\nRESULT label=\(label)")
    for r in rows {
        let v = !r.ok ? "ERROR" : "overlay=\(r.overlay > 500 ? "VISIBLE" : "hidden")(\(r.overlay)) main=\(r.main > 500 ? "VISIBLE" : "hidden")(\(r.main))"
        print("  \(r.method.padding(toLength: 22, withPad: " ", startingAt: 0)) \(v) \(r.note)")
    }
    // machine-readable
    let json = rows.map { "\"\($0.method)\":{\"ok\":\($0.ok),\"overlay\":\($0.overlay),\"main\":\($0.main)}" }.joined(separator: ",")
    try? "{\"label\":\"\(label)\",\(json)}\n".write(toFile: "\(outdir)/\(label).json", atomically: true, encoding: .utf8)
    var pass = true
    for r in rows {
        if !r.ok { print("FAIL \(r.method): capture error \(r.note)"); pass = false; continue }
        if let e = expect["overlay"], e != "any", (r.overlay > 500) != (e == "visible") { print("FAIL \(r.method): overlay expected \(e), saw \(r.overlay) px"); pass = false }
        if let e = expect["main"], e != "any", (r.main > 500) != (e == "visible") { print("FAIL \(r.method): main expected \(e), saw \(r.main) px"); pass = false }
    }
    print(pass ? "PASS \(label)" : "FAILED \(label)")
    return pass
}

// fullscreen dummy window in its own Space, to test overlay above a fullscreen app
final class AppD: NSObject, NSApplicationDelegate {
    var w: NSWindow!
    func applicationDidFinishLaunching(_ n: Notification) {
        w = NSWindow(contentRect: NSRect(x: 200, y: 200, width: 600, height: 400), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        w.title = "fsdummy"; w.backgroundColor = NSColor(red: 0, green: 0, blue: 1, alpha: 1)
        w.collectionBehavior = [.fullScreenPrimary]
        w.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { self.w.toggleFullScreen(nil) }
    }
}

var args = CommandLine.arguments
var expect: [String: String] = [:]
if let i = args.firstIndex(of: "--owner"), i + 1 < args.count { ownerFilter = args[i + 1]; args.removeSubrange(i...i + 1) }
if let i = args.firstIndex(of: "--expect"), i + 1 < args.count {
    for kv in args[i + 1].split(separator: ",") { let p = kv.split(separator: "="); if p.count == 2 { expect[String(p[0])] = String(p[1]) } }
    args.removeSubrange(i...i + 1)
}
if args.count >= 2 && args[1] == "fsdummy" {
    let app = NSApplication.shared; app.setActivationPolicy(.regular)
    let d = AppD(); app.delegate = d; app.run()
} else if args.count >= 3 {
    exit(await runProbe(outdir: args[1], label: args[2], expect: expect) ? 0 : 1)
} else { print("usage: probe <outdir> <label> [--owner NAME] [--expect overlay=hidden,main=visible] | probe fsdummy"); exit(2) }
