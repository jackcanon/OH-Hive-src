import SwiftUI

/// The HaloBench mark, drawn in SwiftUI so it scales anywhere. Same geometry as
/// scripts/make-icon.swift (which produces the Dock icon): honey hexagon "halo" around a bench
/// gauge with a needle.
struct LogoView: View {
    var size: CGFloat = 64
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size * 0.22, style: .continuous)
                .fill(LinearGradient(colors: [Color(red: 0.13, green: 0.14, blue: 0.18), Color(red: 0.06, green: 0.07, blue: 0.09)],
                                     startPoint: .top, endPoint: .bottom))
            Hexagon()
                .stroke(Color.honey, style: StrokeStyle(lineWidth: size * 0.055, lineJoin: .round))
                .frame(width: size * 0.68, height: size * 0.68)
            Gauge(fraction: 0.7)
                .frame(width: size * 0.42, height: size * 0.42)
        }
        .frame(width: size, height: size)
    }

    struct Hexagon: Shape {
        func path(in r: CGRect) -> Path {
            var p = Path()
            let c = CGPoint(x: r.midX, y: r.midY); let R = min(r.width, r.height) / 2
            for i in 0..<6 {
                let a = CGFloat(i) * .pi / 3 + .pi / 6
                let pt = CGPoint(x: c.x + R * cos(a), y: c.y + R * sin(a))
                i == 0 ? p.move(to: pt) : p.addLine(to: pt)
            }
            p.closeSubpath(); return p
        }
    }

    struct Gauge: View {
        var fraction: Double
        var body: some View {
            GeometryReader { g in
                let s = min(g.size.width, g.size.height)
                let c = CGPoint(x: g.size.width / 2, y: g.size.height / 2)
                let R = s / 2
                let start = Angle.degrees(150), end = Angle.degrees(390)
                ZStack {
                    Path { p in p.addArc(center: c, radius: R, startAngle: start, endAngle: end, clockwise: false) }
                        .stroke(Color.gray.opacity(0.7), style: StrokeStyle(lineWidth: s * 0.09, lineCap: .round))
                    Path { p in p.addArc(center: c, radius: R, startAngle: start, endAngle: .degrees(150 + 240 * fraction), clockwise: false) }
                        .stroke(Color.mint, style: StrokeStyle(lineWidth: s * 0.09, lineCap: .round))
                    let na = Angle.degrees(150 + 240 * fraction).radians
                    Path { p in
                        p.move(to: c)
                        p.addLine(to: CGPoint(x: c.x + R * 0.9 * cos(na), y: c.y + R * 0.9 * sin(na)))
                    }.stroke(Color.honey, style: StrokeStyle(lineWidth: s * 0.08, lineCap: .round))
                    Circle().fill(Color.honey).frame(width: s * 0.18, height: s * 0.18)
                }
            }
        }
    }
}

extension Color {
    static let honey = Color(red: 0.96, green: 0.70, blue: 0.26)
}
