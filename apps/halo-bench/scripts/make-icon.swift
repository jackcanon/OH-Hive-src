#!/usr/bin/env swift
// Draws the HaloBench icon with CoreGraphics and writes a full .iconset. Run by build-app.sh:
//   swift scripts/make-icon.swift HaloBench.iconset && iconutil -c icns HaloBench.iconset
// Design: dark rounded square, a honey-coloured hexagon ring ("halo") with a bench-gauge needle
// inside -- the same mark LogoView.swift draws in SwiftUI, so the Dock icon and the in-app logo
// match.
import AppKit

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "HaloBench.iconset"
try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)

func draw(size: CGFloat) -> NSImage {
    let img = NSImage(size: NSSize(width: size, height: size))
    img.lockFocus()
    guard let ctx = NSGraphicsContext.current?.cgContext else { return img }
    let s = size
    let r = CGRect(x: 0, y: 0, width: s, height: s)

    // Background: rounded square, near-black with a subtle vertical gradient.
    let inset = s * 0.05
    let bgRect = r.insetBy(dx: inset, dy: inset)
    let bgPath = CGPath(roundedRect: bgRect, cornerWidth: s * 0.22, cornerHeight: s * 0.22, transform: nil)
    ctx.addPath(bgPath); ctx.clip()
    let colors = [CGColor(red: 0.13, green: 0.14, blue: 0.18, alpha: 1), CGColor(red: 0.06, green: 0.07, blue: 0.09, alpha: 1)] as CFArray
    let grad = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: colors, locations: [0, 1])!
    ctx.drawLinearGradient(grad, start: CGPoint(x: 0, y: s), end: CGPoint(x: 0, y: 0), options: [])

    // Halo: hexagon ring.
    let c = CGPoint(x: s / 2, y: s / 2)
    let R = s * 0.34
    func hex(_ radius: CGFloat) -> CGPath {
        let p = CGMutablePath()
        for i in 0..<6 {
            let a = CGFloat(i) * .pi / 3 + .pi / 6
            let pt = CGPoint(x: c.x + radius * cos(a), y: c.y + radius * sin(a))
            i == 0 ? p.move(to: pt) : p.addLine(to: pt)
        }
        p.closeSubpath(); return p
    }
    ctx.setLineWidth(s * 0.055)
    ctx.setLineJoin(.round)
    ctx.setStrokeColor(CGColor(red: 0.96, green: 0.70, blue: 0.26, alpha: 1))
    ctx.addPath(hex(R)); ctx.strokePath()

    // Gauge arc inside the hexagon (bench dial), from 210deg to -30deg.
    let gR = R * 0.62
    ctx.setLineWidth(s * 0.035)
    ctx.setLineCap(.round)
    ctx.setStrokeColor(CGColor(red: 0.60, green: 0.64, blue: 0.72, alpha: 1))
    ctx.addArc(center: c, radius: gR, startAngle: .pi * 7 / 6, endAngle: -.pi / 6, clockwise: true)
    ctx.strokePath()
    // Filled portion of the gauge (green, ~70%).
    ctx.setStrokeColor(CGColor(red: 0.30, green: 0.76, blue: 0.54, alpha: 1))
    ctx.addArc(center: c, radius: gR, startAngle: .pi * 7 / 6, endAngle: .pi * 0.2, clockwise: true)
    ctx.strokePath()
    // Needle.
    let na: CGFloat = .pi * 0.2
    ctx.setLineWidth(s * 0.03)
    ctx.setStrokeColor(CGColor(red: 0.96, green: 0.70, blue: 0.26, alpha: 1))
    ctx.move(to: c)
    ctx.addLine(to: CGPoint(x: c.x + gR * 0.9 * cos(na), y: c.y + gR * 0.9 * sin(na)))
    ctx.strokePath()
    ctx.setFillColor(CGColor(red: 0.96, green: 0.70, blue: 0.26, alpha: 1))
    ctx.fillEllipse(in: CGRect(x: c.x - s * 0.035, y: c.y - s * 0.035, width: s * 0.07, height: s * 0.07))

    img.unlockFocus()
    return img
}

func writePNG(_ img: NSImage, to path: String) {
    guard let tiff = img.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
          let png = rep.representation(using: .png, properties: [:]) else { return }
    try? png.write(to: URL(fileURLWithPath: path))
}

for (name, px) in [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64),
                   ("128x128", 128), ("128x128@2x", 256), ("256x256", 256), ("256x256@2x", 512),
                   ("512x512", 512), ("512x512@2x", 1024)] {
    writePNG(draw(size: CGFloat(px)), to: "\(outDir)/icon_\(name).png")
}
print("wrote iconset to \(outDir)")
