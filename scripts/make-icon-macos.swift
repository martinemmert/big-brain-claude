// Draws Brain's app icon: a grid of sessions on ink blue, one of them calling.
// Usage: swift scripts/make-icon-macos.swift <out.png> [size]
import AppKit

let args = CommandLine.arguments
let out = args.count > 1 ? args[1] : "icon.png"
let size = CGFloat(args.count > 2 ? Double(args[2])! : 1024)
let s = size / 1024

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(red: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: a)
}

let space = CGColorSpace(name: CGColorSpace.sRGB)!
let ctx = CGContext(data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0,
                    space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!

// macOS icon grid: 824pt squircle centred on a 1024 canvas.
let plate = CGRect(x: 100 * s, y: 100 * s, width: 824 * s, height: 824 * s)
let plateRadius = 186 * s
let platePath = CGPath(roundedRect: plate, cornerWidth: plateRadius, cornerHeight: plateRadius, transform: nil)

// Drop shadow under the plate.
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -14 * s), blur: 40 * s, color: rgb(0x000000, 0.45))
ctx.addPath(platePath)
ctx.setFillColor(rgb(0x0f1322))
ctx.fillPath()
ctx.restoreGState()

// Plate gradient, lighter at the top.
ctx.saveGState()
ctx.addPath(platePath)
ctx.clip()
let plateGradient = CGGradient(colorsSpace: space, colors: [rgb(0x262f4f), rgb(0x0f1322)] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(plateGradient, start: CGPoint(x: 0, y: plate.maxY), end: CGPoint(x: 0, y: plate.minY), options: [])
ctx.restoreGState()

// Hairline highlight along the edge.
ctx.addPath(CGPath(roundedRect: plate.insetBy(dx: 2 * s, dy: 2 * s), cornerWidth: plateRadius - 2 * s,
                   cornerHeight: plateRadius - 2 * s, transform: nil))
ctx.setStrokeColor(rgb(0xffffff, 0.09))
ctx.setLineWidth(4 * s)
ctx.strokePath()

// 3 × 3 sessions. Row 0 is the top row.
enum Cell { case quiet, working, calling }
let grid: [[Cell]] = [
    [.quiet, .working, .calling],
    [.working, .quiet, .quiet],
    [.quiet, .quiet, .working],
]
let pitch = 196 * s
let radius = 62 * s
let centre = CGPoint(x: 512 * s, y: 512 * s)

for (row, cells) in grid.enumerated() {
    for (col, cell) in cells.enumerated() {
        let c = CGPoint(x: centre.x + CGFloat(col - 1) * pitch, y: centre.y - CGFloat(row - 1) * pitch)
        let rect = CGRect(x: c.x - radius, y: c.y - radius, width: radius * 2, height: radius * 2)
        switch cell {
        case .quiet:
            ctx.setFillColor(rgb(0x37405f))
            ctx.fillEllipse(in: rect)
        case .working:
            ctx.setFillColor(rgb(0x5aa9ff))
            ctx.fillEllipse(in: rect)
        case .calling:
            // Halo rings, then the dot with a soft glow.
            for (i, alpha) in [0.10, 0.18].enumerated() {
                let r = radius + CGFloat(2 - i) * 34 * s
                ctx.setFillColor(rgb(0xff5c6c, alpha))
                ctx.fillEllipse(in: CGRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2))
            }
            ctx.saveGState()
            ctx.setShadow(offset: .zero, blur: 50 * s, color: rgb(0xff5c6c, 0.9))
            ctx.setFillColor(rgb(0xff5c6c))
            ctx.fillEllipse(in: rect)
            ctx.restoreGState()
        }
    }
}

let image = ctx.makeImage()!
let rep = NSBitmapImageRep(cgImage: image)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
