import AppKit

let svg = CommandLine.arguments[1]
let out = CommandLine.arguments[2]
let sizes = [16, 32, 64, 128, 256, 512, 1024]
try? FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)
guard let image = NSImage(contentsOfFile: svg) else { fatalError("cannot load \(svg)") }

func render(_ px: Int) -> Data {
    let s = CGFloat(px)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    let shadow = NSShadow()
    shadow.shadowOffset = NSSize(width: 0, height: -s * 0.012)
    shadow.shadowBlurRadius = s * 0.022
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.35)
    shadow.set()
    image.draw(in: NSRect(x: 0, y: 0, width: s, height: s))
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for px in sizes {
    let data = render(px)
    if px <= 512 { try! data.write(to: URL(fileURLWithPath: "\(out)/icon_\(px)x\(px).png")) }
    if px >= 32 { try! data.write(to: URL(fileURLWithPath: "\(out)/icon_\(px / 2)x\(px / 2)@2x.png")) }
}
try! render(1024).write(to: URL(fileURLWithPath: "\(out)/../icon-1024.png"))
