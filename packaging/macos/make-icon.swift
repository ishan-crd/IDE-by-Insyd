// Draws the InsyDE app icon (the design's three-node brain mark on the dark
// panel color) as a 1024px PNG. Usage: swift make-icon.swift out.png
import AppKit

let size: CGFloat = 1024
let img = NSImage(size: NSSize(width: size, height: size))
img.lockFocus()
let ctx = NSGraphicsContext.current!.cgContext
// macOS icon grid: 824px rounded square centered in 1024.
let inset: CGFloat = 100
let rect = CGRect(x: inset, y: inset, width: size - 2 * inset, height: size - 2 * inset)
let path = CGPath(roundedRect: rect, cornerWidth: 185, cornerHeight: 185, transform: nil)
ctx.addPath(path)
ctx.setFillColor(NSColor(srgbRed: 0x1D/255, green: 0x1D/255, blue: 0x1C/255, alpha: 1).cgColor)
ctx.fillPath()
ctx.addPath(path)
ctx.setStrokeColor(NSColor(srgbRed: 0x38/255, green: 0x38/255, blue: 0x36/255, alpha: 1).cgColor)
ctx.setLineWidth(6)
ctx.strokePath()
// Brain mark from the design (16x16 viewBox) scaled into the square.
let s = rect.width / 16 * 0.62
let ox = rect.midX - 8 * s, oy = rect.midY - 8 * s
func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: ox + x * s, y: oy + (16 - y) * s) }
ctx.setStrokeColor(NSColor(srgbRed: 0x9C/255, green: 0x9C/255, blue: 0x97/255, alpha: 1).cgColor)
ctx.setLineWidth(1.1 * s)
ctx.setLineCap(.round)
for (a, b) in [((5.8, 5.4), (7.0, 10.1)), ((10.8, 5.2), (8.9, 10.2)), ((6.0, 4.4), (10.4, 4.4))] {
    ctx.move(to: p(a.0, a.1)); ctx.addLine(to: p(b.0, b.1))
}
ctx.strokePath()
for (x, y, r, hex) in [(4.0, 4.5, 2.0, 0x3B7DD8), (12.0, 4.0, 1.6, 0xD95F6E), (8.0, 12.0, 2.0, 0x2E9E6B)] as [(CGFloat, CGFloat, CGFloat, Int)] {
    let c = NSColor(srgbRed: CGFloat((hex >> 16) & 0xff)/255, green: CGFloat((hex >> 8) & 0xff)/255, blue: CGFloat(hex & 0xff)/255, alpha: 1)
    ctx.setFillColor(c.cgColor)
    let pt = p(x, y)
    ctx.fillEllipse(in: CGRect(x: pt.x - r * s, y: pt.y - r * s, width: 2 * r * s, height: 2 * r * s))
}
img.unlockFocus()
let rep = NSBitmapImageRep(data: img.tiffRepresentation!)!
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
