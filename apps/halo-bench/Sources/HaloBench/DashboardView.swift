import SwiftUI
import Charts

/// Status of the testing at a glance: headline numbers, decode chart across every filed run,
/// the next test up, and the most recent reports.
struct DashboardView: View {
    @Environment(BenchStore.self) private var store
    var goToRun: () -> Void

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                kpis
                HStack(alignment: .top, spacing: 16) {
                    chart.frame(maxWidth: .infinity)
                    nextUp.frame(width: 320)
                }
                recent
            }
            .padding(22)
        }
        .navigationTitle("Dashboard")
        .toolbar { Button { store.reload() } label: { Label("Reload", systemImage: "arrow.clockwise") } }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Project Halo — testing status").font(.title2.bold())
                Text("\(store.reports.count) filed reports in \(store.config.reportsDir)").font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                if let e = store.historyError {
                    Label(e, systemImage: "exclamationmark.triangle.fill").font(.caption).foregroundStyle(.orange).padding(.top, 4).textSelection(.enabled)
                }
            }
            Spacer()
            if store.isLoadingHistory { ProgressView().controlSize(.small); Text("Reading reports…").font(.caption).foregroundStyle(.secondary) }
            else if let l = store.latest { Text("Last run \(l.date.short)").font(.caption).foregroundStyle(.secondary) }
        }
    }

    private var kpis: some View {
        HStack(spacing: 12) {
            KPI(value: "\(store.passCount)", label: "passed", tint: .green)
            KPI(value: "\(store.failCount)", label: "failed", tint: .red)
            KPI(value: store.bestDecode.flatMap { $0.tg128 }.map(ReportStore.num) ?? "—", label: store.bestDecode.map { "best decode tok/s · \($0.model)" } ?? "best decode", tint: .honey)
            KPI(value: "\(store.reports.filter { !$0.tensorSplit.isEmpty }.count)", label: "pooled (split) runs", tint: .blue)
            KPI(value: "\(store.reports.filter { !$0.tensorSplit.isEmpty && $0.outcome == .pass }.count)", label: "pooled runs that passed", tint: .mint)
        }
    }

    private var chart: some View {
        let pts = store.reports.filter { $0.tg128 != nil }.sorted { $0.date < $1.date }.suffix(24)
        return GroupBox("Decode tok/s by run") {
            if pts.isEmpty {
                Text("No numeric results yet.").foregroundStyle(.secondary).frame(maxWidth: .infinity, minHeight: 200)
            } else {
                let labeled = Array(pts.enumerated())
                Chart(labeled, id: \.offset) { i, r in
                    BarMark(x: .value("Run", "#\(i + 1)"), y: .value("tg128", r.tg128 ?? 0))
                        .foregroundStyle(r.tensorSplit.isEmpty ? Color.blue : Color.mint)
                        .annotation(position: .top) { Text(ReportStore.num(r.tg128 ?? 0)).font(.caption2) }
                }
                .frame(minHeight: 220)
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 10) {
                        ForEach(labeled, id: \.offset) { i, r in
                            Text("#\(i + 1) \(r.title)").font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                }
                HStack(spacing: 14) {
                    Label("single node", systemImage: "square.fill").foregroundStyle(.blue)
                    Label("pooled (RPC split)", systemImage: "square.fill").foregroundStyle(.mint)
                }.font(.caption).padding(.top, 4)
            }
        }
    }

    private var nextUp: some View {
        GroupBox("Next test up") {
            VStack(alignment: .leading, spacing: 8) {
                let p = Presets.next(given: store.reports)
                Text(p.name).font(.headline)
                Text(p.why).font(.caption).foregroundStyle(.secondary)
                Button { store.apply(p); goToRun() } label: { Label("Load in Run", systemImage: "play.fill") }
                    .buttonStyle(.borderedProminent).tint(.honey)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var recent: some View {
        GroupBox("Recent reports") {
            if store.reports.isEmpty {
                Text("Nothing filed yet. Run a test, or import a log from the Run tab.").foregroundStyle(.secondary).padding(6)
            } else {
                VStack(spacing: 0) {
                    ForEach(store.reports.prefix(8)) { r in
                        ReportRow(report: r)
                        Divider()
                    }
                }
            }
        }
    }
}

struct KPI: View {
    var value: String; var label: String; var tint: Color
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(value).font(.system(size: 26, weight: .bold, design: .rounded)).foregroundStyle(tint)
            Text(label).font(.caption).foregroundStyle(.secondary).lineLimit(2)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 10))
    }
}

struct ReportRow: View {
    var report: TestReport
    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: report.outcome.symbol).foregroundStyle(report.outcome.color)
            VStack(alignment: .leading, spacing: 1) {
                Text(report.title).font(.body)
                Text("\(report.date.short) · \(report.placement.isEmpty ? report.model : report.placement)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            if let tg = report.tg128 { Text("\(ReportStore.num(tg)) tok/s").font(.system(.body, design: .monospaced)) }
        }
        .padding(.vertical, 6)
    }
}
