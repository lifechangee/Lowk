import AppKit
let a = CommandLine.arguments
let x = Double(a[1])!, y = Double(a[2])!, w = Double(a[3])!, h = Double(a[4])!
let app = NSApplication.shared; app.setActivationPolicy(.accessory)
let screenH = NSScreen.screens[0].frame.height
let win = NSWindow(contentRect: NSRect(x: x - 40, y: screenH - (y + h) - 40, width: w + 80, height: h + 80), styleMask: .borderless, backing: .buffered, defer: false)
win.level = .normal
let v = NSView(frame: win.contentView!.bounds); v.wantsLayer = true
let l = CALayer(); l.frame = v.bounds; l.backgroundColor = NSColor.white.cgColor
let d = CALayer(); d.frame = NSRect(x: 0, y: 0, width: v.bounds.width/2, height: v.bounds.height); d.backgroundColor = NSColor.black.cgColor
v.layer!.addSublayer(l); v.layer!.addSublayer(d); win.contentView = v
win.orderFrontRegardless()
app.run()
