import SwiftUI

/// The run's milestones as a horizontal track: done (green), active (pulsing honey), failed (red),
/// pending (grey). Shows overall elapsed time, per-stage durations, and the failure reason under
/// the stage that died.
struct RunTimelineView: View {
    @Environment(BenchStore.self) private var store

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { ctx in
            let tl = store.timeline
            VStack(alignment: .leading, spacing: 10) {
                header(tl, now: ctx.date)
                track(tl, now: ctx.date)
                if let f = tl.failedStage, let d = tl.stages[f]?.detail, !d.isEmpty {
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: "xmark.octagon.fill").foregroundStyle(.red)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Failed during \(f.title)").font(.callout.bold())
                            Text(d).font(.caption).textSelection(.enabled)
                        }
                    }
                    .padding(10)
                    .background(Color.red.opacity(0.12), in: RoundedRectangle(cornerRadius: 8))
                } else if let c = tl.current, store.phase == .running {
                    Text(c.hint).font(.caption).foregroundStyle(.secondary)
                }
            }
            .padding(12)
            .frame(maxWidth: .infinity)
            .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 12))
        }
    }

    private func header(_ tl: Timeline, now: Date) -> some View {
        HStack(spacing: 10) {
            switch store.phase {
            case .running:
                PulseDot(color: .honey)
                Text("Running — \(tl.current?.title ?? "starting")").font(.headline)
            case .finished(let o):
                Image(systemName: o.symbol).foregroundStyle(o.color)
                Text("Finished — \(o.label)").font(.headline)
            case .idle:
                Image(systemName: "circle.dashed").foregroundStyle(.secondary)
                Text("No run yet").font(.headline)
            }
            Spacer()
            if tl.startedAt != nil {
                Text(Self.fmt(tl.elapsed)).font(.system(.title3, design: .monospaced).weight(.semibold))
                    .foregroundStyle(store.phase == .running ? Color.honey : .secondary)
            }
            if store.phase == .running, let last = store.lastOutputAt {
                let quiet = now.timeIntervalSince(last)
                Text(quiet < 3 ? "output live" : "quiet \(Int(quiet))s")
                    .font(.caption).foregroundStyle(quiet < 3 ? Color.green : .secondary)
                    .padding(.horizontal, 8).padding(.vertical, 3)
                    .background(.quaternary.opacity(0.5), in: Capsule())
            }
        }
    }

    private func track(_ tl: Timeline, now: Date) -> some View {
        HStack(alignment: .top, spacing: 0) {
            ForEach(Stage.allCases) { s in
                let st = tl.stages[s] ?? StageState()
                VStack(spacing: 6) {
                    HStack(spacing: 0) {
                        Rectangle().fill(lineColor(before: s, tl)).frame(height: 2).opacity(s == .workers ? 0 : 1)
                        marker(st)
                        Rectangle().fill(lineColor(after: s, tl)).frame(height: 2).opacity(s == .report ? 0 : 1)
                    }
                    Text(s.title).font(.caption.weight(st.status == .active ? .bold : .regular))
                        .foregroundStyle(st.status == .pending || st.status == .skipped ? Color.secondary : .primary)
                        .lineLimit(1)
                    Text(sub(st)).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                }
                .frame(maxWidth: .infinity)
            }
        }
    }

    private func marker(_ st: StageState) -> some View {
        ZStack {
            Circle().fill(color(st)).frame(width: 18, height: 18)
            switch st.status {
            case .done: Image(systemName: "checkmark").font(.system(size: 10, weight: .bold)).foregroundStyle(.black)
            case .failed: Image(systemName: "xmark").font(.system(size: 10, weight: .bold)).foregroundStyle(.white)
            case .active: PulseDot(color: .honey)
            case .skipped: Image(systemName: "minus").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
            case .pending: EmptyView()
            }
        }
    }

    private func color(_ st: StageState) -> Color {
        switch st.status {
        case .done: .green; case .failed: .red; case .active: .honey.opacity(0.35); case .skipped: .gray.opacity(0.3); case .pending: .gray.opacity(0.35)
        }
    }
    private func lineColor(before s: Stage, _ tl: Timeline) -> Color {
        guard let prev = Stage(rawValue: s.rawValue - 1), let p = tl.stages[prev] else { return .clear }
        return (p.status == .done || p.status == .skipped) ? .green : .gray.opacity(0.35)
    }
    private func lineColor(after s: Stage, _ tl: Timeline) -> Color {
        (tl.stages[s]?.status == .done || tl.stages[s]?.status == .skipped) ? .green : .gray.opacity(0.35)
    }
    private func sub(_ st: StageState) -> String {
        switch st.status {
        case .pending: return ""
        case .skipped: return "n/a"
        case .active: return st.duration.map(Self.fmt) ?? ""
        case .done, .failed:
            let d = st.duration.map(Self.fmt) ?? ""
            return st.detail.isEmpty ? d : "\(d) · \(st.detail)"
        }
    }

    static func fmt(_ t: TimeInterval) -> String {
        let s = Int(t); return s >= 60 ? String(format: "%d:%02d", s / 60, s % 60) : "\(s)s"
    }
}

/// A small breathing dot -- the "something is alive" signal.
struct PulseDot: View {
    var color: Color
    @State private var on = false
    var body: some View {
        Circle().fill(color).frame(width: 10, height: 10)
            .scaleEffect(on ? 1.25 : 0.8).opacity(on ? 1 : 0.55)
            .animation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true), value: on)
            .onAppear { on = true }
    }
}
