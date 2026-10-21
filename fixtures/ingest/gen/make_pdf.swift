// Generates the PDF/PNG fixtures of fixtures/ingest (macOS only, run once; outputs are committed).
// usage: swift make_pdf.swift <out-dir>
import AppKit
import CoreGraphics
import Foundation

let out = URL(fileURLWithPath: CommandLine.arguments[1])
let W: CGFloat = 595, H: CGFloat = 842

func draw(_ ctx: CGContext, _ text: String, x: CGFloat, y: CGFloat, size: CGFloat, bold: Bool = false, width: CGFloat = 480) {
    let font = bold ? NSFont.boldSystemFont(ofSize: size) : NSFont.systemFont(ofSize: size)
    let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: CGColor(gray: 0, alpha: 1)]
    let s = NSAttributedString(string: text, attributes: attrs)
    let fs = CTFramesetterCreateWithAttributedString(s)
    let path = CGPath(rect: CGRect(x: x, y: y - 400, width: width, height: 400), transform: nil)
    let frame = CTFramesetterCreateFrame(fs, CFRange(location: 0, length: 0), path, nil)
    CTFrameDraw(frame, ctx)
}

func page(_ ctx: CGContext, _ n: Int, _ blocks: [(String, CGFloat, Bool)]) {
    ctx.beginPDFPage(nil)
    draw(ctx, "Lowk Fixture Corp. - Confidential", x: 56, y: H - 40, size: 9)
    var y = H - 100
    for (t, size, bold) in blocks {
        draw(ctx, t, x: 56, y: y, size: size, bold: bold)
        let lines = max(1, Int(ceil(Double(t.count) * Double(size) * 0.5 / 480.0)))
        y -= CGFloat(lines) * (size * 1.4) + 14
    }
    draw(ctx, "Page \(n)", x: W / 2 - 15, y: 40, size: 9)
    ctx.endPDFPage()
}

func makeTextPDF(_ name: String, pages: Int) {
    var box = CGRect(x: 0, y: 0, width: W, height: H)
    let ctx = CGContext(out.appendingPathComponent(name) as CFURL, mediaBox: &box, nil)!
    let ru = "Поставщик обязуется передать товар покупателю в срок до пятнадцатого ноября две тысячи двадцать шестого года. Стоимость партии составляет 1 250 000 рублей, оплата производится в два этапа."
    let en = "The supplier shall deliver the goods to the buyer no later than November 15, 2026. The total price of the batch is 25,000 USD, payable in two instalments."
    for i in 1...pages {
        switch i % 3 {
        case 1: page(ctx, i, [("Договор поставки № \(i)", 20, true), (ru, 12, false), ("Supply agreement section \(i)", 16, true), (en, 12, false)])
        case 2: page(ctx, i, [("Delivery terms and penalties", 18, true), (en, 12, false), ("Условия доставки и штрафы", 18, true), (ru, 12, false)])
        default: page(ctx, i, [("Приложение: контрольная фраза квазар-\(i)", 16, true), ("Mixed paragraph: код проекта LOWK-\(1000 + i) и срок 30 days после подписания.", 12, false)])
        }
    }
    ctx.closePDF()
}

func renderPNG(_ text: [String], size: CGSize, to url: URL) -> CGImage {
    let cs = CGColorSpaceCreateDeviceRGB()
    let ctx = CGContext(data: nil, width: Int(size.width), height: Int(size.height), bitsPerComponent: 8, bytesPerRow: 0, space: cs, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
    ctx.setFillColor(CGColor(gray: 1, alpha: 1)); ctx.fill(CGRect(origin: .zero, size: size))
    var y = size.height - 90
    for t in text {
        draw(ctx, t, x: 40, y: y, size: 34, width: size.width - 80)
        y -= 70
    }
    let img = ctx.makeImage()!
    let rep = NSBitmapImageRep(cgImage: img)
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
    return img
}

makeTextPDF("text.pdf", pages: 3)

let lines = ["Акт приёмки № 77", "Scanned invoice total 4200 EUR", "Оплата до 1 декабря"]
let img = renderPNG(lines, size: CGSize(width: 1400, height: 420), to: out.appendingPathComponent("scan.png"))
// image-only (scanned) PDF, 2 pages
var box = CGRect(x: 0, y: 0, width: 700, height: 210)
let sctx = CGContext(out.appendingPathComponent("scanned.pdf") as CFURL, mediaBox: &box, nil)!
for _ in 0..<2 { sctx.beginPDFPage(nil); sctx.draw(img, in: box); sctx.endPDFPage() }
sctx.closePDF()
