import AppKit

let out = CommandLine.arguments[1]
let sizes = [16, 32, 64, 128, 256, 512, 1024]
try? FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)

func render(_ px: Int) -> Data {
    let s = CGFloat(px)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let inset = s * 0.09
    let rect = NSRect(x: inset, y: inset, width: s - inset * 2, height: s - inset * 2)
    let path = NSBezierPath(roundedRect: rect, xRadius: rect.width * 0.225, yRadius: rect.width * 0.225)
    NSGradient(colors: [NSColor(red: 0.20, green: 0.23, blue: 0.27, alpha: 1), NSColor(red: 0.05, green: 0.07, blue: 0.09, alpha: 1)])!.draw(in: path, angle: -90)
    let config = NSImage.SymbolConfiguration(pointSize: s * 0.42, weight: .bold).applying(.init(paletteColors: [NSColor(red: 0.25, green: 0.73, blue: 0.31, alpha: 1)]))
    if let sym = NSImage(systemSymbolName: "arrow.triangle.pull", accessibilityDescription: nil)?.withSymbolConfiguration(config) {
        let sz = sym.size
        sym.draw(in: NSRect(x: (s - sz.width) / 2, y: (s - sz.height) / 2, width: sz.width, height: sz.height))
    }
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for px in sizes {
    let data = render(px)
    if px <= 512 { try! data.write(to: URL(fileURLWithPath: "\(out)/icon_\(px)x\(px).png")) }
    if px >= 32 { try! data.write(to: URL(fileURLWithPath: "\(out)/icon_\(px / 2)x\(px / 2)@2x.png")) }
}
