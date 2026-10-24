// Generates fixtures/ingest/encrypted.pdf (user password "secret"). macOS only; output is committed.
// usage: swift make_encrypted_pdf.swift <out-dir>
import AppKit
import CoreGraphics
import Foundation

let out = URL(fileURLWithPath: CommandLine.arguments[1]).appendingPathComponent("encrypted.pdf")
var box = CGRect(x: 0, y: 0, width: 595, height: 842)
let aux: [CFString: Any] = [
    kCGPDFContextUserPassword: "secret",
    kCGPDFContextOwnerPassword: "admin-secret",
]
let ctx = CGContext(out as CFURL, mediaBox: &box, aux as CFDictionary)!
ctx.beginPDFPage(nil)
let attrs: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 14), .foregroundColor: CGColor(gray: 0, alpha: 1)]
let s = NSAttributedString(string: "Секретный договор. Confidential contract text for the encryption test.", attributes: attrs)
let fs = CTFramesetterCreateWithAttributedString(s)
let frame = CTFramesetterCreateFrame(fs, CFRange(location: 0, length: 0), CGPath(rect: CGRect(x: 56, y: 600, width: 480, height: 200), transform: nil), nil)
CTFrameDraw(frame, ctx)
ctx.endPDFPage()
ctx.closePDF()
